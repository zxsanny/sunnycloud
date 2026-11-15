//! The composition root over a real PostgreSQL 18 database (a template clone) and a scratch blob volume, with a
//! complete `serve` configuration: what `process_env` builds, with the test database's API URL.

#![allow(dead_code)]

use std::fs;
use std::sync::Arc;

use sc_clock::{Clock, SystemClock};
use sc_config::{ConfigSources, load_keys};
use sc_testdb::TestDb;
use sunnycloud_server::root;
use sunnycloud_server::{CliError, ConfigCheck, ServeSettings, ServiceContext};

#[path = "../config_support/mod.rs"]
mod config_support;

use config_support::Setup;

/// Distinct interval per janitor key, so the test can tell which key each janitor reads.
pub const INTERVALS: &[(&str, &str)] = &[
    ("session_reap_interval", "11m"),
    ("receipt_janitor_interval", "12m"),
    ("checkpoint_interval", "13s"),
    ("gc_interval", "14m"),
    ("orphan_sweep_interval", "15h"),
    ("scrub_interval", "16h"),
    ("tree_maintenance_interval", "17s"),
    ("account_delete_interval", "18s"),
    ("job_reap_interval", "19m"),
    ("derived_janitor_interval", "20m"),
    ("identity_janitor_interval", "21m"),
    ("audit_retention_interval", "22m"),
];

pub struct Root {
    pub services: ServiceContext,
    pub db: TestDb,
    setup: Setup,
}

impl Root {
    pub async fn new() -> Root {
        Root::with_intervals(INTERVALS).await
    }

    /// Every janitor interval key set to its value in `intervals`.
    pub async fn with_intervals(intervals: &[(&str, &str)]) -> Root {
        let db = TestDb::clone_template().await;
        let mut setup = Setup::valid(0);
        let blob_root = setup.dir.join("blobs");
        fs::create_dir_all(&blob_root).expect("blob volume");
        let key = setup.dir.join("master.key");
        fs::write(&key, [0x5au8; 32]).expect("key file");
        let api_url = db.config().db_api_url.expose().clone();
        setup
            .set("environment", "test")
            .set("blob_root", &blob_root.display().to_string())
            .secret("db_api_url", &api_url)
            .secret("master_key_file", &key.display().to_string());
        for (key, value) in intervals {
            setup.set(key, value);
        }
        let services = context(ConfigSources {
            env: setup.env.clone().into_iter().collect(),
            ..ConfigSources::default()
        });
        Root {
            services,
            db,
            setup,
        }
    }

    /// Stops the scheduler when it was started, then drops the database.
    pub async fn finish(self) {
        let Root {
            services,
            db,
            setup,
        } = self;
        if services
            .built()
            .contains(&std::any::type_name::<sunnycloud_server::janitor::Scheduler>())
        {
            let scheduler = services
                .get::<sunnycloud_server::janitor::Scheduler>()
                .await
                .expect("scheduler");
            scheduler.stop().await;
        }
        drop(services);
        drop(setup);
        db.drop_db().await;
    }
}

fn context(sources: ConfigSources) -> ServiceContext {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let sources = Arc::new(sources);
    let builder = ServiceContext::builder()
        .register::<ConfigSources, _, _>(move |_| {
            let sources = Arc::clone(&sources);
            async move { Ok(sources) }
        })
        .register::<ServeSettings, _, _>(|services| async move {
            let sources = services.get::<ConfigSources>().await?;
            load_keys::<ServeSettings>(&sources, &ConfigCheck::Server.keys())
                .map_err(CliError::from)
        });
    root::register(builder, clock).finish()
}

/// The thin address of an `Arc`, to compare a trait object with the concrete value it was made from.
pub fn address<T: ?Sized>(item: &Arc<T>) -> *const () {
    Arc::as_ptr(item).cast::<()>()
}
