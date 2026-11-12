//! The tree's part of the account lifecycle (`AccountLifecycleHook`) and of blob collection (`BlobOwnerCleanup`).
//! Neither takes the account advisory lock: a deleting account has no writer left, and the GC sweeper locks
//! blob rows only.

use std::num::TryFromIntError;
use std::sync::Arc;

use contracts::{
    AccountLifecycleHook, BlobCleanupError, BlobOwnerCleanup, Due, EraseProgress, GcQueue,
    GcReason, LifecycleError, async_trait,
};
use sc_clock::Clock;
use sc_db::{Db, DbError, DbTx, Scope};
use sc_ids::new_id;
use uuid::Uuid;

use crate::lifecycle_repo;
use crate::retry::run_retrying_each;

/// Registered by 12 in the hook list (after 20's vault hook, whose rows reference tree nodes) and in the GC
/// sweeper's cleanup list. Separate from `PgTreeService` because it needs the GC queue and none of the request
/// path's ports.
pub struct PgTreeLifecycle {
    db: Db,
    queue: Arc<dyn GcQueue>,
    clock: Arc<dyn Clock>,
}

impl PgTreeLifecycle {
    pub fn new(db: Db, queue: Arc<dyn GcQueue>, clock: Arc<dyn Clock>) -> Self {
        PgTreeLifecycle { db, queue, clock }
    }

    /// One batch: the revisions with their blobs queued, then leaves level by level with what is left of `limit`.
    /// Each blob row is locked by its queue write before its revision is deleted, the order the GC sweeper uses
    /// (blob row, then `remove_owner_rows`), so the two never wait on each other in a cycle.
    async fn erase_in(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        limit: u32,
    ) -> Result<(i64, i64), LifecycleError> {
        let limit = u64::from(limit);
        let owned = lifecycle_repo::revision_batch(tx, account, limit)
            .await
            .map_err(unexpected)?;
        for row in &owned {
            self.queue
                .schedule_in_tx(tx, row.blob_id, GcReason::AccountDeleted, Due::Now)
                .await
                .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        }
        let ids: Vec<Uuid> = owned.iter().map(|row| row.revision_id).collect();
        let deleted = lifecycle_repo::delete_revisions(tx, account, &ids)
            .await
            .map_err(unexpected)?;
        let mut budget = limit.saturating_sub(deleted);
        while budget > 0 {
            let leaves = lifecycle_repo::delete_leaves(tx, account, budget)
                .await
                .map_err(unexpected)?;
            if leaves == 0 {
                break;
            }
            budget = budget.saturating_sub(leaves);
        }
        lifecycle_repo::tree_rows(tx, account)
            .await
            .map_err(unexpected)
    }
}

#[async_trait]
impl AccountLifecycleHook for PgTreeLifecycle {
    /// Inserts the account root in the account-creation transaction; a second root is refused by the database.
    async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError> {
        let id = new_id(self.clock.as_ref());
        lifecycle_repo::insert_root(tx, id, account, self.clock.now_utc())
            .await
            .map_err(unexpected)
    }

    /// One short transaction per call, retried once on a deadlock. `remaining` also counts the account's blobs
    /// still queued, so it reaches 0 only after the sweeper removed their files.
    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        let (nodes, revisions) = run_retrying_each(&self.db, Scope::Account(account), || {
            async move |tx: &mut DbTx| self.erase_in(tx, account, limit).await
        })
        .await?;
        let queued = self
            .queue
            .pending_for_account(account)
            .await
            .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        let remaining = remaining(nodes, revisions, queued)
            .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        Ok(EraseProgress { remaining })
    }
}

#[async_trait]
impl BlobOwnerCleanup for PgTreeLifecycle {
    /// Deletes the revision that points at the blob, current or not; a blob with no revision is a no-op.
    async fn remove_owner_rows(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
    ) -> Result<(), BlobCleanupError> {
        lifecycle_repo::delete_owner(tx, blob_id)
            .await
            .map_err(|error| BlobCleanupError::Unexpected(Box::new(error)))
    }
}

/// Nodes, revisions and queued blobs of the account; a negative count from the database is a failure.
fn remaining(nodes: i64, revisions: i64, queued: u64) -> Result<u64, TryFromIntError> {
    Ok(u64::try_from(nodes)?
        .saturating_add(u64::try_from(revisions)?)
        .saturating_add(queued))
}

fn unexpected(error: DbError) -> LifecycleError {
    LifecycleError::Unexpected(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_is_the_sum_of_nodes_revisions_and_queued_blobs() {
        // Arrange
        let (nodes, revisions, queued) = (3, 5, 7);

        // Act
        let total = remaining(nodes, revisions, queued);

        // Assert
        assert_eq!(total, Ok(15));
    }

    #[test]
    fn a_negative_count_is_a_failure() {
        // Arrange
        let (nodes, revisions, queued) = (-1, 0, 0);

        // Act
        let total = remaining(nodes, revisions, queued);

        // Assert
        assert!(total.is_err());
    }
}
