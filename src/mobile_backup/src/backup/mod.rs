//! The backup loop (17 §5): one run per `Backup`, new assets from the change token and the visible-id diff stored
//! as rows, hashes saved ahead of the uploads, uploads through `sync-core` with stable keys. A kill at any point
//! leaves a checkpoint the next run continues from; nothing is ever marked `uploaded` before its acknowledgement.

mod discovery;
mod folder;
mod pipeline;
mod sink;
mod unreadable;
mod verdict;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sync_core::{BatchSink, SyncError};
use sync_core_proto::BatchEntry;
use tracing::warn;
use uuid::Uuid;

use crate::api::BackupApi;
use crate::checkpoint::{ACCESS_LEVEL_KEY, AssetStore, CheckpointDb, SqliteCheckpointStore};
use crate::config::AndroidConfig;
use crate::outcome::{BackupOutcome, PauseReason, RunContext, RunError, SkipReason};
use crate::source::{AccessLevel, AssetSource, OpenError, ReadableFd};
use pipeline::Run;
use unreadable::Unreadable;
use verdict::Halt;

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

    /// Ends the run in progress `Partial`: its next check stops, and the sync client ends the operations in flight
    /// with `Cancelled`. That client stays cancelled for good, so later runs need a new `Backup` over a new client.
    /// Without a run in progress this does nothing.
    pub fn cancel(&self) {
        if self.running.load(Ordering::Acquire) {
            self.cancel.store(true, Ordering::Release);
            self.api.cancel();
        }
    }

    /// Runs the backup unless another run of this `Backup` is in progress, then answers `Skipped(AlreadyRunning)`
    /// without any call to the sync API. Pauses and the errors of single assets are settled inside
    /// (`backup_core.md` § Wire errors); the `Err` cases are the ones the host must see, `Sync(Unauthorized)` being
    /// `TokenRejected`.
    pub async fn run_backup(
        &self,
        ctx: RunContext,
        source: Arc<dyn AssetSource>,
    ) -> Result<BackupOutcome, RunError> {
        let Some(_guard) = RunGuard::acquire(&self.running) else {
            return Ok(BackupOutcome::Skipped(SkipReason::AlreadyRunning));
        };
        self.cancel.store(false, Ordering::Release);
        self.assets.begin_run()?;
        let outcome = match self.run(&ctx, &source).await {
            Ok(outcome) => outcome,
            Err(Halt::Pause(reason)) => BackupOutcome::Paused(reason),
            Err(Halt::Error(RunError::Sync(SyncError::Cancelled))) => self.partial()?,
            Err(Halt::Error(error)) => return Err(error),
        };
        if let BackupOutcome::Paused(reason) = outcome {
            self.assets.set_paused(reason.as_text())?;
        }
        Ok(outcome)
    }

    async fn run(
        &self,
        ctx: &RunContext,
        source: &Arc<dyn AssetSource>,
    ) -> Result<BackupOutcome, Halt> {
        if let Some(reason) = ctx.network_policy.pause() {
            return Ok(BackupOutcome::Paused(reason));
        }
        let level = source.access_level()?;
        if level == AccessLevel::Denied {
            self.assets
                .set_cursor_text(ACCESS_LEVEL_KEY, level.as_text())?;
            return Ok(BackupOutcome::Paused(PauseReason::PermissionDenied));
        }
        self.resume_batches(source).await?;
        self.discover(ctx, source.as_ref(), level)?;
        Run::new(self, *ctx, Arc::clone(source)).work().await
    }

    fn partial(&self) -> Result<BackupOutcome, RunError> {
        Ok(BackupOutcome::Partial {
            remaining: self.assets.pending_count()?,
        })
    }

    /// Finishes the batches an earlier run staged but did not store the results of.
    async fn resume_batches(&self, source: &Arc<dyn AssetSource>) -> Result<(), Halt> {
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
            // `None` is the resolver's answer for a file that is gone; a host failure reads as an unreadable file,
            // which sync-core hands back as `Io` and the loop ends `failed`.
            match source.open_read(id) {
                Ok(opened) => Some(opened.file),
                Err(OpenError::Gone) => None,
                Err(OpenError::Failed) => Some(Arc::new(Unreadable) as ReadableFd),
            }
        };
        let sink = Arc::clone(&self.assets) as Arc<dyn BatchSink>;
        let left_out = self
            .api
            .resume_batches(self.store.clone(), sink, &resolve)
            .await
            .map_err(|error| self.halted(error))?;
        self.settle_left_out(left_out)
    }
}
