//! 03's part of the account lifecycle (`AccountLifecycleHook`, AC-27, AC-30). A new account has no transfer rows.
//! The erase, driven by 07's purger once the account is `deleting` (starts, stages and commits are then refused by
//! the account guard), removes the account's session rows, staged rows, committed-key records and commit receipts
//! in one short account-scoped transaction per call, without the account advisory lock.
//!
//! Session rows go through the reaper's statement (`session_repo::delete_rows`, every row instead of the expired
//! ones), and each open row's blob is queued `AccountDeleted`, `Due::Now` in the same transaction, so no row goes
//! without its `blob_gc` row; 04's sweeper removes the file. A committed-key record's blob is its revision's and
//! goes with 02's erase. No reservation is released: 07a erases the account's `quota` row. Progress entries are
//! evicted after COMMIT.

use std::sync::Arc;

use contracts::{
    AccountLifecycleHook, Due, EraseProgress, Fault, GcQueue, GcReason, LifecycleError, async_trait,
};
use sc_db::{DbTx, Scope};
use thiserror::Error;
use uuid::Uuid;

use crate::reaper::run_fault;
use crate::receipt_repo;
use crate::session_repo::{self, DeletedRow};
use crate::upload::PgUploadService;

#[derive(Debug, Error)]
#[error("negative row count {0}")]
struct NegativeCount(i64);

/// Registered by 12 in the hook list. Holds the upload service, whose database and progress table the erase uses,
/// and 04's GC queue.
pub struct PgTransferLifecycle {
    service: Arc<PgUploadService>,
    gc: Arc<dyn GcQueue>,
}

impl PgTransferLifecycle {
    pub fn new(service: Arc<PgUploadService>, gc: Arc<dyn GcQueue>) -> Self {
        PgTransferLifecycle { service, gc }
    }

    /// In the caller's transaction: up to `limit` session rows deleted with their open blobs queued, then
    /// receipts with the rest of `limit`; returns the deleted session rows and what is left.
    async fn erase_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        limit: i64,
    ) -> Result<(Vec<DeletedRow>, u64), Fault> {
        let rows = session_repo::delete_rows(tx, account, None, None, limit).await?;
        for row in rows.iter().filter(|row| row.open) {
            self.gc
                .schedule_in_tx(tx, row.blob_id, GcReason::AccountDeleted, Due::Now)
                .await?;
        }
        let deleted = i64::try_from(rows.len()).unwrap_or(i64::MAX);
        let budget = receipt_budget(limit, deleted);
        if budget > 0 {
            receipt_repo::delete_account(tx, account, budget).await?;
        }
        let (sessions, receipts) = receipt_repo::account_rows(tx, account).await?;
        Ok((rows, remaining(sessions, receipts)?))
    }
}

/// What is left of the call's `limit` for receipts once `sessions_deleted` session rows went.
fn receipt_budget(limit: i64, sessions_deleted: i64) -> i64 {
    limit.saturating_sub(sessions_deleted).max(0)
}

/// The account's session rows plus its receipts.
fn remaining(sessions: i64, receipts: i64) -> Result<u64, NegativeCount> {
    let sessions = u64::try_from(sessions).map_err(|_| NegativeCount(sessions))?;
    let receipts = u64::try_from(receipts).map_err(|_| NegativeCount(receipts))?;
    Ok(sessions.saturating_add(receipts))
}

#[async_trait]
impl AccountLifecycleHook for PgTransferLifecycle {
    /// A new account has no transfer rows: no statement, no I/O.
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// One short account-scoped transaction per call: up to `limit` rows, session rows first, then receipts.
    /// `remaining` counts the account's `upload_sessions` and `commit_receipts` rows left after the call; an
    /// account with none, erased or unknown, reports 0 again.
    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        let (rows, remaining) = self
            .service
            .db
            .run(Scope::Account(account), async |tx| {
                self.erase_in_tx(tx, account, i64::from(limit)).await
            })
            .await
            .map_err(|error| LifecycleError::Unexpected(run_fault(error)))?;
        for row in &rows {
            self.service.evict(row.id);
        }
        Ok(EraseProgress { remaining })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_is_the_session_rows_plus_the_receipts_left() {
        // Arrange
        let cases = [(0, 0, 0), (30, 0, 30), (0, 5000, 5000), (230, 4900, 5130)];

        // Act
        let sums: Vec<u64> = cases
            .iter()
            .map(|&(sessions, receipts, _)| remaining(sessions, receipts).expect("counts"))
            .collect();

        // Assert
        let expected: Vec<u64> = cases.iter().map(|&(_, _, sum)| sum).collect();
        assert_eq!(sums, expected);
    }

    #[test]
    fn a_negative_count_is_a_failure() {
        // Arrange
        let counts = [(-1, 0), (0, -1)];

        // Act
        let results: Vec<bool> = counts
            .iter()
            .map(|&(sessions, receipts)| remaining(sessions, receipts).is_err())
            .collect();

        // Assert
        assert_eq!(results, [true, true]);
    }

    #[test]
    fn session_rows_take_the_budget_first_and_receipts_get_the_rest() {
        // Arrange
        let limit = 100;
        let cases = [(100, 0), (30, 70), (0, 100), (130, 0)];

        // Act
        let budgets: Vec<i64> = cases
            .iter()
            .map(|&(sessions, _)| receipt_budget(limit, sessions))
            .collect();

        // Assert
        let expected: Vec<i64> = cases.iter().map(|&(_, receipts)| receipts).collect();
        assert_eq!(budgets, expected);
    }
}
