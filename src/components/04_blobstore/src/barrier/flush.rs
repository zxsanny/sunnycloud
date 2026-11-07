use std::fs::File;
use std::sync::Arc;

use uuid::Uuid;

use super::state::{Outcome, Shared, SyncFailure};

/// The flush of one group, run as its own task so that a cancelled waiter cannot strand the others. The sync
/// itself runs on the blocking pool.
pub(super) async fn run(shared: Arc<Shared>) {
    if !shared.delay.is_zero() {
        tokio::time::sleep(shared.delay).await;
    }
    let (group, files) = shared.close_open_group();
    let worker = Arc::clone(&shared);
    let outcome = match tokio::task::spawn_blocking(move || worker.sync_group(&files)).await {
        Ok(outcome) => outcome,
        Err(_) => Err(SyncFailure {
            kind: std::io::ErrorKind::Other,
            blob_id: None,
        }),
    };
    shared.publish(group, outcome);
}

impl Shared {
    fn sync_group(&self, files: &[(Uuid, Arc<File>)]) -> Outcome {
        #[cfg(feature = "sync-hook")]
        if let Some(hook) = &self.hook {
            return hook.run(files);
        }
        sync_files(files, |_| {})
    }
}

/// `fdatasync` on every file, stopping at the first failure. `before` sees each file just before its sync.
pub(super) fn sync_files(files: &[(Uuid, Arc<File>)], mut before: impl FnMut(Uuid)) -> Outcome {
    for (blob_id, file) in files {
        before(*blob_id);
        file.sync_data().map_err(|error| SyncFailure {
            kind: error.kind(),
            blob_id: Some(*blob_id),
        })?;
    }
    Ok(())
}
