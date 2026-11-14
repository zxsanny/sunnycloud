//! The `commit_receipts` statements of `ReceiptEffect`, run on 02's commit transaction under the account lock, and
//! of the account erase (`crate::lifecycle`), with the count of the account's transfer rows the erase reports.
//! Every statement names the account, so receipts are per account (AC-59).

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::Hash32;
use uuid::Uuid;

const FIND: &str = "SELECT request_hash, response::text FROM commit_receipts \
     WHERE account_id = $1 AND commit_id = $2";

const INSERT: &str = "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
     VALUES ($1, $2, $3, $4::text::jsonb, 'epoch'::timestamptz + $5::bigint * interval '1 microsecond')";

// Up to `$2` receipts of the account; one a running batch commit holds is skipped and counted as remaining. The
// claim is materialised so a rescan cannot pick more than `$2` rows (`session_repo::DELETE_ROWS`).
const DELETE_ACCOUNT: &str = "WITH picked AS MATERIALIZED ( \
     SELECT commit_id FROM commit_receipts WHERE account_id = $1 \
     ORDER BY commit_id LIMIT $2 FOR UPDATE SKIP LOCKED) \
     DELETE FROM commit_receipts r USING picked WHERE r.account_id = $1 AND r.commit_id = picked.commit_id";

const REMAINING: &str = "SELECT (SELECT count(*) FROM upload_sessions WHERE account_id = $1), \
     (SELECT count(*) FROM commit_receipts WHERE account_id = $1)";

/// A stored receipt: the request hash and the serialised `BatchResult`.
pub(crate) struct Receipt {
    pub request_hash: Vec<u8>,
    pub response: String,
}

pub(crate) async fn find(
    tx: &mut DbTx,
    account: Uuid,
    commit_id: Uuid,
) -> Result<Option<Receipt>, DbError> {
    let row: Option<(Vec<u8>, String)> = sqlx::query_as(FIND)
        .bind(account)
        .bind(commit_id)
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(|(request_hash, response)| Receipt {
        request_hash,
        response,
    }))
}

pub(crate) async fn insert(
    tx: &mut DbTx,
    account: Uuid,
    commit_id: Uuid,
    request_hash: &Hash32,
    response: &str,
    now: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(INSERT)
        .bind(account)
        .bind(commit_id)
        .bind(request_hash.as_bytes().as_slice())
        .bind(response)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// Deletes up to `limit` receipts of `account`; returns how many went.
pub(crate) async fn delete_account(
    tx: &mut DbTx,
    account: Uuid,
    limit: i64,
) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_ACCOUNT)
        .bind(account)
        .bind(limit)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// The account's `upload_sessions` rows (open, staged and committed-key records) and its receipts.
pub(crate) async fn account_rows(tx: &mut DbTx, account: Uuid) -> Result<(i64, i64), DbError> {
    Ok(sqlx::query_as(REMAINING)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}
