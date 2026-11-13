//! The statements of the registry. Each runs on the caller's transaction and is filtered by the caller's account
//! (and, in tenant scope, by RLS too), so an id of another account behaves like an absent row.

use sc_db::{DbError, DbTx};
use uuid::Uuid;

const INSERT: &str = "INSERT INTO blobs \
     (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
     VALUES ($1, $2, $3, $4, $5, $6, 'epoch'::timestamptz + $7::bigint * interval '1 microsecond')";

const COMMIT: &str = "UPDATE blobs SET state = 'committed', size = $3 \
     WHERE id = $1 AND account_id = $2 AND state = 'uploading'";

const LOCK_STATE: &str = "SELECT state FROM blobs WHERE id = $1 AND account_id = $2 FOR UPDATE";

const DELETE_QUEUE_ROW: &str = "DELETE FROM blob_gc WHERE blob_id = $1 AND account_id = $2";

const SET_COMMITTED: &str =
    "UPDATE blobs SET state = 'committed' WHERE id = $1 AND account_id = $2";

const DELETE_BLOB: &str = "DELETE FROM blobs WHERE id = $1 AND account_id = $2";

pub(super) struct NewRow<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub state: &'static str,
    pub size: Option<i64>,
    pub wrapped_key: &'a [u8],
    pub master_key_id: i16,
    pub created_micros: i64,
}

pub(super) async fn insert(tx: &mut DbTx, row: NewRow<'_>) -> Result<(), DbError> {
    sqlx::query(INSERT)
        .bind(row.id)
        .bind(row.account)
        .bind(row.state)
        .bind(row.size)
        .bind(row.wrapped_key)
        .bind(row.master_key_id)
        .bind(row.created_micros)
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// True when an `uploading` row became `committed`.
pub(super) async fn commit(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    size: i64,
) -> Result<bool, DbError> {
    let done = sqlx::query(COMMIT)
        .bind(id)
        .bind(account)
        .bind(size)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

/// The state of the row, locked until the transaction ends.
pub(super) async fn lock_state(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar(LOCK_STATE)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?)
}

/// Removes the queue row and returns the blob to `committed`.
pub(super) async fn restore(tx: &mut DbTx, account: Uuid, id: Uuid) -> Result<(), DbError> {
    sqlx::query(DELETE_QUEUE_ROW)
        .bind(id)
        .bind(account)
        .execute(tx.conn())
        .await?;
    sqlx::query(SET_COMMITTED)
        .bind(id)
        .bind(account)
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// The queue row goes with the blob row (`ON DELETE CASCADE`).
pub(super) async fn delete(tx: &mut DbTx, account: Uuid, id: Uuid) -> Result<(), DbError> {
    sqlx::query(DELETE_BLOB)
        .bind(id)
        .bind(account)
        .execute(tx.conn())
        .await?;
    Ok(())
}
