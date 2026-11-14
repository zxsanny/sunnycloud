//! 08's part of the account lifecycle (`AccountLifecycleHook`) and of blob collection (`BlobOwnerCleanup`). The
//! erase removes the account's jobs, then its derived rows with their blobs queued `Due::Now`; the cleanup removes
//! the derived row of a blob the GC sweeper collects, inside the sweeper's transaction.

use std::sync::Arc;

use contracts::{
    AccountLifecycleHook, BlobCleanupError, BlobOwnerCleanup, EraseProgress, Fault, GcQueue,
    GcReason, LifecycleError, async_trait,
};
use sc_db::{Db, DbTx, RunError, Scope};
use uuid::Uuid;

use crate::maintenance_repo;

/// Registered by 12 in the hook list and in the GC sweeper's cleanup list.
pub struct MediaLifecycle {
    db: Db,
    gc: Arc<dyn GcQueue>,
}

impl MediaLifecycle {
    pub fn new(db: Db, gc: Arc<dyn GcQueue>) -> Self {
        MediaLifecycle { db, gc }
    }
}

/// What is left of the call's `limit` for derived rows once `jobs_deleted` jobs went.
fn derived_budget(limit: u64, jobs_deleted: u64) -> u64 {
    limit.saturating_sub(jobs_deleted)
}

/// The fault of a failed transaction, whichever side failed.
pub(crate) fn fault(error: RunError<Fault>) -> Fault {
    match error {
        RunError::Closure(fault) => fault,
        RunError::Db(error) => Box::new(error),
    }
}

#[async_trait]
impl AccountLifecycleHook for MediaLifecycle {
    /// A new account has no media rows.
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// One short account-scoped transaction: up to `limit` rows, jobs first, then derived rows, each derived blob
    /// queued `Due::Now` (`AccountDeleted`) before its row goes. `remaining` counts the account's jobs and derived
    /// rows left after the call; an account with none, erased or unknown, reports 0 again.
    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        let limit = u64::from(limit);
        let remaining = self
            .db
            .run(Scope::Account(account), async |tx| {
                let jobs = maintenance_repo::delete_account_jobs(tx, account, limit).await?;
                let budget = derived_budget(limit, jobs);
                if budget > 0 {
                    let blobs = maintenance_repo::lock_account_derived(tx, account, budget).await?;
                    maintenance_repo::queue_and_delete(
                        tx,
                        self.gc.as_ref(),
                        &blobs,
                        GcReason::AccountDeleted,
                    )
                    .await?;
                }
                Ok::<_, Fault>(maintenance_repo::remaining(tx, account).await?)
            })
            .await
            .map_err(|error| LifecycleError::Unexpected(fault(error)))?;
        Ok(EraseProgress { remaining })
    }
}

#[async_trait]
impl BlobOwnerCleanup for MediaLifecycle {
    /// Deletes the derived row that owns the blob; a blob with no derived row is a no-op. No transaction of its
    /// own, no file I/O, no other port.
    async fn remove_owner_rows(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
    ) -> Result<(), BlobCleanupError> {
        maintenance_repo::delete_derived(tx, &[blob_id])
            .await
            .map(drop)
            .map_err(|error| BlobCleanupError::Unexpected(Box::new(error)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_take_the_budget_first_and_derived_rows_get_the_rest() {
        // Arrange
        let limit = 50;
        let cases = [(50, 0), (20, 30), (0, 50), (70, 0)];

        // Act
        let budgets: Vec<u64> = cases
            .iter()
            .map(|&(jobs, _)| derived_budget(limit, jobs))
            .collect();

        // Assert
        let expected: Vec<u64> = cases.iter().map(|&(_, derived)| derived).collect();
        assert_eq!(budgets, expected);
    }
}
