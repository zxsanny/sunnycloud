//! The rig of every test: a fake source, a fake API over a fake server, the real SQLite checkpoint in a temp
//! directory, and the `Backup` under test.

pub mod api;
pub mod db;
pub mod log;
pub mod record;
pub mod server;
pub mod source;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mobile_backup::{
    AndroidConfig, Backup, BackupCore, BackupOutcome, CheckpointDb, HttpProbe, NetworkPolicy,
    RunContext, RunError, Trigger,
};
use tempfile::TempDir;
use uuid::Uuid;

use api::FakeApi;
use db::Db;
use server::Server;
use source::FakeSource;

pub const DEVICE: Uuid = Uuid::from_u128(0xDE71_CE00_0000_4000_8000_0000_0000_0001);
pub const LABEL: &str = "Pixel 9";

pub fn ctx(trigger: Trigger) -> RunContext {
    RunContext {
        trigger,
        deadline: None,
        network_policy: NetworkPolicy::default(),
    }
}

pub struct Rig {
    _dir: TempDir,
    pub path: PathBuf,
    pub db: Db,
    pub source: Arc<FakeSource>,
    pub server: Arc<Server>,
    pub api: Arc<FakeApi>,
    pub backup: Arc<Backup>,
    /// The `status` and `cancel` of the surface, over the checkpoint the loop writes.
    pub core: Arc<BackupCore>,
    hash_ahead: u32,
    batch_max: usize,
}

fn backup_over(
    db: &Arc<CheckpointDb>,
    api: &Arc<FakeApi>,
    hash_ahead: u32,
) -> (Arc<Backup>, Arc<BackupCore>) {
    let backup = Arc::new(Backup::new(
        Arc::clone(api) as _,
        Arc::clone(db),
        AndroidConfig {
            catchup_interval: Duration::from_secs(15 * 60),
            unmetered_only: true,
            charging_only: false,
            hash_ahead,
        },
        DEVICE,
        LABEL.to_owned(),
    ));
    let core = BackupCore::with_probe(Arc::clone(db), "1.0.0".to_owned(), Arc::new(HttpProbe));
    core.attach(Arc::clone(&backup));
    (backup, core)
}

impl Rig {
    pub fn new(hash_ahead: u32, batch_max: usize) -> Self {
        Self::over(
            Arc::new(FakeSource::default()),
            Arc::new(Server::default()),
            hash_ahead,
            batch_max,
        )
    }

    /// Another process of the same app: its own checkpoint file, the same photos and the same server.
    pub fn second_process(&self) -> Self {
        Self::over(
            Arc::clone(&self.source),
            Arc::clone(&self.server),
            self.hash_ahead,
            self.batch_max,
        )
    }

    fn over(
        source: Arc<FakeSource>,
        server: Arc<Server>,
        hash_ahead: u32,
        batch_max: usize,
    ) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("checkpoint.db");
        let db = Db::new(&path);
        let api = FakeApi::new(Arc::clone(&server), batch_max);
        api.watch(db.clone());
        let (backup, core) = backup_over(&Arc::new(CheckpointDb::new(&path)), &api, hash_ahead);
        Self {
            _dir: dir,
            path,
            db,
            source,
            server,
            api,
            backup,
            core,
            hash_ahead,
            batch_max,
        }
    }

    /// A process restart: a new `Backup` and a new API object over the same checkpoint file and server.
    pub fn restart(&mut self) {
        self.api = FakeApi::new(Arc::clone(&self.server), self.batch_max);
        self.api.watch(self.db.clone());
        let (backup, core) = backup_over(
            &Arc::new(CheckpointDb::new(&self.path)),
            &self.api,
            self.hash_ahead,
        );
        self.backup = backup;
        self.core = core;
    }

    pub async fn try_run(&self, trigger: Trigger) -> Result<BackupOutcome, RunError> {
        self.try_run_with(ctx(trigger)).await
    }

    pub async fn try_run_with(&self, ctx: RunContext) -> Result<BackupOutcome, RunError> {
        self.backup.run_backup(ctx, self.source.clone()).await
    }

    pub async fn run(&self, trigger: Trigger) -> BackupOutcome {
        self.try_run(trigger)
            .await
            .expect("the run ends with an outcome")
    }
}
