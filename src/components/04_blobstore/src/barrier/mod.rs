//! The in-process group-commit barrier: writers register the file they wrote, the first waiter starts a flush
//! that `fdatasync`s every dirty file of the group, and all waiters of the group are released together. Writers
//! that arrive during a flush join the next group. There is no timer acknowledgement: a waiter is released only
//! by the outcome of a real sync.

mod flush;
#[cfg(feature = "sync-hook")]
mod hook;
mod state;

use std::fs::File;
use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, DurableToken};
use uuid::Uuid;

use crate::error::blob_io_failure;
use state::Shared;

#[cfg(feature = "sync-hook")]
pub use hook::{SyncHook, SyncRecord};

#[derive(Clone)]
pub struct SyncBarrier {
    shared: Arc<Shared>,
}

impl SyncBarrier {
    /// `coalesce_delay` is the wait between the first waiter and the start of the flush (`blob_sync_coalesce_delay`,
    /// default zero).
    pub fn new(coalesce_delay: Duration) -> Self {
        SyncBarrier {
            shared: Arc::new(Shared::new(coalesce_delay)),
        }
    }

    #[cfg(feature = "sync-hook")]
    pub fn with_hook(coalesce_delay: Duration, hook: Arc<SyncHook>) -> Self {
        SyncBarrier {
            shared: Arc::new(Shared::with_hook(coalesce_delay, hook)),
        }
    }

    /// Records that `file` holds a completed write. It joins the open group, whose sync starts after this call
    /// returns, so the sync covers the write. Registering the same blob again in one group syncs it once.
    pub fn register(&self, blob_id: Uuid, file: Arc<File>) -> DurableToken {
        DurableToken(self.shared.register(blob_id, file))
    }

    /// `Ok` only after an `fdatasync` that started after the write completed. Every waiter of a group whose sync
    /// failed gets `Io`; the next group is unaffected. A token is awaited once: an unknown or already awaited
    /// token is `Io`, never `Ok`.
    pub async fn await_durable(&self, token: DurableToken) -> Result<(), BlobError> {
        let _release = Release {
            shared: &self.shared,
            group: token.0,
        };
        loop {
            let notified = self.shared.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(outcome) = self.shared.check(token.0) {
                return outcome.map_err(|failure| blob_io_failure(failure.kind, failure.ids()));
            }
            notified.await;
        }
    }
}

struct Release<'a> {
    shared: &'a Arc<Shared>,
    group: u64,
}

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.shared.release(self.group);
    }
}
