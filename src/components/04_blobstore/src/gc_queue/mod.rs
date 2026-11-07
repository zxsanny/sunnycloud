//! `GcQueue`: the single writer of `blob_gc` and of the `gc_pending` state.

mod gc_repo;

use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, Due, GcQueueError, GcReason, async_trait};
use sc_clock::Clock;
use sc_db::{Db, DbTx};
use uuid::Uuid;

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, db_fault, row_fault, run_fault};

pub struct GcQueue {
    db: Db,
    clock: Arc<dyn Clock>,
    retention_micros: i64,
}

fn reason_text(reason: GcReason) -> &'static str {
    match reason {
        GcReason::Replaced => "replaced",
        GcReason::Deleted => "deleted",
        GcReason::AccountDeleted => "account_deleted",
        GcReason::DerivedRemoved => "derived_removed",
    }
}

impl GcQueue {
    /// Refuses a retention that does not fit in microseconds of an `i64`; the configured range is 0 to 90 days.
    pub fn new(config: &BlobstoreConfig, db: Db, clock: Arc<dyn Clock>) -> Result<Self, BlobError> {
        let retention_micros = retention_micros(config.retention).ok_or_else(|| {
            crate::error::blob_error_from_io(
                std::io::Error::from(std::io::ErrorKind::InvalidInput),
                BlobIds::default(),
            )
        })?;
        Ok(GcQueue {
            db,
            clock,
            retention_micros,
        })
    }

    fn due_micros(&self, due: Due) -> i64 {
        let now = self.clock.now_utc().as_unix_micros();
        match due {
            Due::AfterRetention => now.saturating_add(self.retention_micros),
            Due::Now => now,
        }
    }
}

fn retention_micros(retention: Duration) -> Option<i64> {
    i64::try_from(retention.as_micros()).ok()
}

#[async_trait]
impl contracts::GcQueue for GcQueue {
    /// A blob with no visible row (absent, or another account's) is a failure and writes nothing.
    async fn schedule_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
        reason: GcReason,
        due: Due,
    ) -> Result<(), GcQueueError> {
        let ids = BlobIds {
            blob_id: Some(blob_id),
            account_id: None,
        };
        match gc_repo::schedule(tx, blob_id, self.due_micros(due), reason_text(reason)).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(GcQueueError::Unexpected(row_fault("no blob row", ids))),
            Err(error) => Err(GcQueueError::Unexpected(db_fault(error, ids))),
        }
    }

    async fn pending_for_account(&self, account: Uuid) -> Result<u64, GcQueueError> {
        let ids = BlobIds {
            blob_id: None,
            account_id: Some(account),
        };
        let count = gc_repo::pending(&self.db, account)
            .await
            .map_err(|error| GcQueueError::Unexpected(run_fault(error, ids)))?;
        u64::try_from(count).map_err(|_| GcQueueError::Unexpected(row_fault("negative count", ids)))
    }
}
