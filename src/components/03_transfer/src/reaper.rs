//! Expiry of session and staged rows, and the sum of their reservations (`upload_service.md`).
//!
//! A row is gone once `now` reaches its `expires_at` (`session_repo::is_expired`, rolling from the last
//! checkpoint). Whoever deletes a row settles its reservation, so the reaper deletes in the same short transaction
//! in which it releases: the account advisory lock first (02's `publish` takes it before the quota row and the
//! session row, so a commit and a reap of one account run one after the other and never deadlock), then
//! `DELETE … RETURNING`, `QuotaLedger::release_reservation` for exactly the returned sum and
//! `BlobRegistry::remove_row_in_tx` for each open row. A commit that comes second finds no open row and rolls
//! back its own release; a reap that comes second finds the committed record, whose expiry is new. Files are
//! discarded and progress entries evicted after COMMIT. `start_upload` and `stage_small` reap an expired row under
//! their key the same way before they reuse the key.
//!
//! The cycle needs no leader: it lists the accounts with expired rows in system scope (`janitor_repo`) and reaps
//! each in account scope, in batches; a second reaper waits for the advisory lock and then finds nothing.

use contracts::{Fault, ReservationError};
use sc_clock::Timestamp;
use sc_db::{DbTx, RunError, Scope};
use sc_observability::log_error;
use uuid::Uuid;

use crate::janitor_repo;
use crate::session_repo::{self, ExpiredRow};
use crate::upload::PgUploadService;
use crate::upload_metrics::SessionResult;

/// Accounts one listing of the reaper returns.
const ACCOUNT_PAGE: i64 = 100;

/// Rows one reaper transaction deletes.
const REAP_BATCH: i64 = 100;

impl PgUploadService {
    /// In the caller's transaction: the account advisory lock, the rows of `account` (of `client_key` alone when
    /// given) expired at `now`, at most `limit`, the release of exactly their reservations and the blob rows of
    /// the open ones. The caller hands the rows to [`Self::reaped`] after COMMIT.
    pub(crate) async fn reap_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        client_key: Option<&str>,
        now: Timestamp,
        limit: i64,
    ) -> Result<Vec<ExpiredRow>, Fault> {
        tx.advisory_lock_account(account).await?;
        let rows = session_repo::delete_expired(tx, account, client_key, now, limit).await?;
        let reserved = rows
            .iter()
            .fold(0i64, |sum, row| sum.saturating_add(row.reserved_bytes));
        if reserved > 0 {
            self.quota
                .release_reservation(tx, account, reserved)
                .await?;
        }
        for row in rows.iter().filter(|row| row.open) {
            self.registry
                .remove_row_in_tx(tx, account, row.blob_id)
                .await?;
        }
        Ok(rows)
    }

    /// After COMMIT of a reap: evicts the progress entries, discards the files of the open rows (best effort: a
    /// file left behind has no row and is 04's orphan sweeper's) and counts each expired large session.
    pub(crate) async fn reaped(&self, account: Uuid, rows: &[ExpiredRow]) {
        for row in rows {
            self.evict(row.id);
            if !row.open {
                continue;
            }
            if let Err(error) = self.writer.discard(row.blob_id).await {
                log_error!(
                    "expired upload file not discarded",
                    account_id = account,
                    err = &error
                );
            }
            if let Some(metrics) = self.metrics.as_ref().filter(|_| row.large) {
                metrics.session_ended(SessionResult::Expired);
            }
        }
    }

    /// One reaper cycle: every account with rows expired at `now`, in batches. Returns the rows deleted.
    pub(crate) async fn reap_expired(&self, now: Timestamp) -> Result<u64, Fault> {
        let mut processed = 0u64;
        let mut after = None;
        loop {
            let accounts =
                janitor_repo::expired_accounts(&self.db, now, after, ACCOUNT_PAGE).await?;
            for &account in &accounts {
                processed = processed.saturating_add(self.reap_account(account, now).await?);
            }
            if i64::try_from(accounts.len()).unwrap_or(i64::MAX) < ACCOUNT_PAGE {
                return Ok(processed);
            }
            after = accounts.last().copied();
        }
    }

    async fn reap_account(&self, account: Uuid, now: Timestamp) -> Result<u64, Fault> {
        let mut processed = 0u64;
        loop {
            let rows = self
                .db
                .run(Scope::Account(account), async |tx| {
                    self.reap_in_tx(tx, account, None, now, REAP_BATCH).await
                })
                .await
                .map_err(run_fault)?;
            self.reaped(account, &rows).await;
            processed = processed.saturating_add(rows.len() as u64);
            if i64::try_from(rows.len()).unwrap_or(i64::MAX) < REAP_BATCH {
                return Ok(processed);
            }
        }
    }

    /// `ReservationSource::open_reserved_bytes`: the sum of `reserved_bytes` of the account's open sessions and
    /// staged rows, expired ones not yet reaped included (07a holds their reservation until the reaper releases
    /// it). A committed record holds none.
    pub async fn open_reserved_bytes(&self, account: Uuid) -> Result<i64, ReservationError> {
        self.db
            .run(Scope::Account(account), async |tx| {
                session_repo::open_reserved(tx, account).await
            })
            .await
            .map_err(|error| ReservationError::Unexpected(Box::new(error)))
    }
}

/// A transaction that failed in its closure or around it.
pub(crate) fn run_fault(error: RunError<Fault>) -> Fault {
    match error {
        RunError::Closure(fault) => fault,
        RunError::Db(error) => Box::new(error),
    }
}
