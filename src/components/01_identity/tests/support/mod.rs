//! Component test harness (01 `tests.md`, Test Data Management) on a real PostgreSQL 18 (`DATABASE_URL`, a
//! superuser URL).
//!
//! The database is a clone of the migrated template of `sc_testdb` (the two roles, every embedded migration
//! applied by `sc_db::Migrator`). The component under test connects as `sunnycloud`, fixtures and probes as
//! `sunnycloud_migrator`, time comes from a `ManualClock`, and the providers point at the URL of the mock
//! identity provider (`MOCK_IDP_URL`; the service itself is built by the test epic).

#![allow(dead_code, unused_imports)]

pub mod accounts;
pub mod audit;
pub mod auth;
pub mod device;
pub mod fixtures;
pub mod geoip;
pub mod invites;
pub mod logs;
pub mod onboarding;
pub mod provider;
pub mod registration;
mod secrets;
pub mod sessions;
pub mod sign_in;

use std::sync::Arc;

use sc_audit::AuditWriter;
use sc_clock::{Clock, ManualClock};
use sc_config::{ApiSettings, ConfigSources, load};
use sc_db::Db;
pub use sc_testdb::{API, MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use sunnycloud_identity::IdentityConfig;

pub const DEFAULT_MOCK_IDP_URL: &str = "http://mock-idp:9000";

/// Durations of the harness section: milliseconds and seconds, never the production values.
pub const SHORT_DURATIONS: [(&str, &str); 5] = [
    ("SUNNYCLOUD_SESSION_CACHE_TTL", "200ms"),
    ("SUNNYCLOUD_REFRESH_REUSE_GRACE", "300ms"),
    ("SUNNYCLOUD_OAUTH_STATE_TTL", "1s"),
    ("SUNNYCLOUD_LOGIN_CODE_TTL", "1s"),
    ("SUNNYCLOUD_DEVICE_CODE_TTL", "2s"),
];

pub fn mock_idp_url() -> String {
    std::env::var("MOCK_IDP_URL").unwrap_or_else(|_| DEFAULT_MOCK_IDP_URL.to_owned())
}

/// Loads the API settings the way the server does, with short millisecond durations (`test` environment),
/// and `extra` environment pairs on top.
pub fn load_settings(extra: &[(&str, &str)]) -> Arc<ApiSettings> {
    let mut sources = ConfigSources {
        secrets_dir: secrets::shared_dir(),
        environment: sc_config::Environment::Test,
        ..ConfigSources::default()
    };
    let required = [
        ("SUNNYCLOUD_PUBLIC_URL", "https://sunnycloud.test"),
        ("SUNNYCLOUD_GITHUB_CLIENT_ID", "gh-client"),
    ];
    for (key, value) in required.iter().chain(extra) {
        sources.env.insert((*key).to_owned(), (*value).to_owned());
    }
    load::<ApiSettings>(&sources)
        .unwrap_or_else(|error| panic!("the harness settings do not load: {error}"))
}

/// A valid identity section from `settings`, its providers pointed at the mock identity provider.
pub fn identity_config(settings: &ApiSettings) -> IdentityConfig {
    let mut config = IdentityConfig::from_settings(settings);
    let url = mock_idp_url();
    for provider in &mut config.providers {
        provider.issuer = Some(url.clone());
    }
    config
}

/// The component under test on role `sunnycloud`, the manual clock and the identity section.
pub struct Harness {
    pub db: TestDb,
    pub api: Db,
    pub clock: Arc<ManualClock>,
    pub config: IdentityConfig,
}

impl Harness {
    pub async fn new() -> Harness {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let config = identity_config(&load_settings(&SHORT_DURATIONS));
        Harness {
            db,
            api,
            clock: sc_testdb::clock_at_start(),
            config,
        }
    }

    /// The audit writer of the component, on its pool and clock.
    pub fn audit(&self) -> AuditWriter {
        AuditWriter::new(self.api.clone(), self.clock.clone())
    }

    pub fn now_micros(&self) -> i64 {
        self.clock.now_utc().as_unix_micros()
    }

    /// Fixtures and probes run as the schema owner.
    pub async fn fixture_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Releases the component's connections and drops the database. Returns the number of connections that
    /// were still open when the pool was released, after waiting for the server to see them close.
    pub async fn finish(self) -> i64 {
        let Harness { db, api, .. } = self;
        drop(api);
        db.finish().await
    }
}
