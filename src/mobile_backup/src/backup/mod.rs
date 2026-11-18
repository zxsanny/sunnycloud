//! The backup loop (17 §5): one run per `Backup`, new assets from the change token and the visible-id diff stored
//! as rows, hashes saved ahead of the uploads, uploads through `sync-core` with stable keys. A kill at any point
//! leaves a checkpoint the next run continues from; nothing is ever marked `uploaded` before its acknowledgement.

mod discovery;
mod folder;
mod pipeline;
mod sink;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sync_core::BatchSink;
use sync_core_proto::BatchEntry;
use tracing::warn;
use uuid::Uuid;

use crate::api::BackupApi;
use crate::checkpoint::{AssetStore, CheckpointDb, SqliteCheckpointStore};
use crate::config::AndroidConfig;
use crate::outcome::{BackupOutcome, PauseReason, RunContext, RunError, SkipReason};
use crate::source::{AccessLevel, AssetSource};
use pipeline::Run;

pub struct Backup {
    api: Arc<dyn BackupApi>,
    assets: Arc<AssetStore>,
    store: Arc<SqliteCheckpointStore>,
    config: AndroidConfig,
    device_id: Uuid,
    device_label: String,
    running: AtomicBool,
    cancel: AtomicBool,
}

/// Holds the run flag of one `Backup` and releases it when the run ends, however it ends.
struct RunGuard<'a>(&'a AtomicBool);

impl<'a> RunGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        let taken = flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire);
        taken.is_ok().then(|| Self(flag))
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Backup {
    /// The checkpoint database opens on the first run, not here. `device_id` is the account's id of this device, the
    /// same in every process of the app: it is the first input of every `client_key`. Keep one `Backup` per
    /// process: the run flag is its own, and a second one over the same database would not exclude the first.
    pub fn new(
        api: Arc<dyn BackupApi>,
        db: Arc<CheckpointDb>,
        config: AndroidConfig,
        device_id: Uuid,
        device_label: String,
    ) -> Self {
        Self {
            api,
            assets: Arc::new(AssetStore::new(Arc::clone(&db))),
            store: Arc::new(SqliteCheckpointStore::new(db)),
            config,
            device_id,
            device_label,
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    /// Asks the run in progress to end `Partial` at its next check; a run that starts later is not affected.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Runs the backup unless another run of this `Backup` is in progress, then answers `Skipped(AlreadyRunning)`
    /// without any call to the sync API.
    pub async fn run_backup(
        &self,
        ctx: RunContext,
        source: Arc<dyn AssetSource>,
    ) -> Result<BackupOutcome, RunError> {
        let Some(_guard) = RunGuard::acquire(&self.running) else {
            return Ok(BackupOutcome::Skipped(SkipReason::AlreadyRunning));
        };
        self.cancel.store(false, Ordering::Release);
        let level = source.access_level();
        if level == AccessLevel::Denied {
            return Ok(BackupOutcome::Paused(PauseReason::PermissionDenied));
        }
        self.resume_batches(&source).await?;
        self.discover(&ctx, source.as_ref(), level)?;
        Run::new(self, ctx, source).drive().await
    }

    /// Finishes the batches an earlier run staged but did not store the results of.
    async fn resume_batches(&self, source: &Arc<dyn AssetSource>) -> Result<(), RunError> {
        let assets = Arc::clone(&self.assets);
        let source = Arc::clone(source);
        let resolve = move |entry: &BatchEntry| {
            let id = match assets.asset_of_key(entry.staged_key) {
                Ok(id) => id?,
                Err(error) => {
                    warn!(%error, "a staged batch entry could not be matched to its asset");
                    return None;
                }
            };
            // `None` is the resolver's answer for a file that is gone, which is the only `open_read` error.
            source.open_read(id).ok()
        };
        let sink = Arc::clone(&self.assets) as Arc<dyn BatchSink>;
        let left_out = self
            .api
            .resume_batches(self.store.clone(), sink, &resolve)
            .await?;
        for file in left_out {
            warn!(error = %file.error, "a staged file could not be replayed");
        }
        Ok(())
    }
}
