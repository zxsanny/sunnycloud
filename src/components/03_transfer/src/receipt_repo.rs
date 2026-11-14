//! The `commit_receipts` statements of `ReceiptEffect`, run on 02's commit transaction under the account lock.
//! Every statement names the account, so receipts are per account (AC-59).

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::Hash32;
use uuid::Uuid;

const FIND: &str = "SELECT request_hash, response::text FROM commit_receipts \
     WHERE account_id = $1 AND commit_id = $2";

const INSERT: &str = "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
     VALUES ($1, $2, $3, $4::text::jsonb, 'epoch'::timestamptz + $5::bigint * interval '1 microsecond')";

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
