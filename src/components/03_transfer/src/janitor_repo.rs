//! The two statements of 03's janitors that cross accounts, the only `run_system` call sites of 03: the accounts
//! that hold expired session rows (the reaper then works in account scope) and the deletion of old commit
//! receipts (a receipt has no reservation, file or entry to settle). Both only read or delete rows past a time the
//! caller computed from its own clock.

use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError};
use uuid::Uuid;

// One page of accounts in id order, after `$2` when it is set; the `expires_at` index finds the rows.
const EXPIRED_ACCOUNTS: &str = "SELECT DISTINCT account_id FROM upload_sessions \
     WHERE expires_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
     AND ($2::uuid IS NULL OR account_id > $2) ORDER BY account_id LIMIT $3";

// Receipts created before `$1`, at most `$2`; a receipt another janitor holds is skipped.
// The picked set is materialised once, so a rescan can never delete more than `$2` rows.
const DELETE_OLD_RECEIPTS: &str = "WITH picked AS MATERIALIZED ( \
     SELECT account_id, commit_id FROM commit_receipts \
     WHERE created_at < 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
     LIMIT $2 FOR UPDATE SKIP LOCKED) \
     DELETE FROM commit_receipts r USING picked \
     WHERE r.account_id = picked.account_id AND r.commit_id = picked.commit_id";

/// Up to `limit` accounts with a session or staged row expired at `now`, in id order after `after`.
pub(crate) async fn expired_accounts(
    db: &Db,
    now: Timestamp,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Uuid>, DbError> {
    db.run_system("transfer_expired_accounts", async |tx| {
        sqlx::query_scalar(EXPIRED_ACCOUNTS)
            .bind(now.as_unix_micros())
            .bind(after)
            .bind(limit)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)
    })
    .await
    .map_err(flatten)
}

/// Deletes up to `limit` receipts created before `before`; returns how many went.
pub(crate) async fn delete_old_receipts(
    db: &Db,
    before: Timestamp,
    limit: i64,
) -> Result<u64, DbError> {
    db.run_system("transfer_receipt_janitor", async |tx| {
        sqlx::query(DELETE_OLD_RECEIPTS)
            .bind(before.as_unix_micros())
            .bind(limit)
            .execute(tx.conn())
            .await
            .map(|done| done.rows_affected())
            .map_err(DbError::from)
    })
    .await
    .map_err(flatten)
}

fn flatten(error: RunError<DbError>) -> DbError {
    match error {
        RunError::Closure(error) | RunError::Db(error) => error,
    }
}
