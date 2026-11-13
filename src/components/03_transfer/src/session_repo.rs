//! The `upload_sessions` statements of `start_upload` and of the `put_block` checkpoint. Every statement names the account, so a row of another
//! account is never read or written even in system scope. Timestamps travel as Unix microseconds.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

/// The unique key that makes a start idempotent by `(account, client_key)`.
pub(crate) const CLIENT_KEY_INDEX: &str = "upload_sessions_account_id_client_key_key";

const BY_KEY: &str = "SELECT id, declared_size, block_count, received, \
     (extract(epoch FROM expires_at) * 1000000)::bigint, committed_revision_id \
     FROM upload_sessions WHERE account_id = $1 AND client_key = $2";

const DELETE_EXPIRED_COMMITTED: &str = "DELETE FROM upload_sessions \
     WHERE account_id = $1 AND client_key = $2 AND committed_revision_id IS NOT NULL \
     AND expires_at <= 'epoch'::timestamptz + $3::bigint * interval '1 microsecond'";

// Open large-path sessions: not committed, and either more than one block (a staged row has 0 or 1) or one
// block not yet received (a staged row is inserted with its block received). A one-block large session whose
// block has arrived is indistinguishable from a staged row until a column tells them apart.
const COUNT_OPEN_LARGE: &str = "SELECT count(*) FROM upload_sessions \
     WHERE account_id = $1 AND committed_revision_id IS NULL \
     AND (block_count > 1 OR received = '\\x00'::bytea)";

const INSERT: &str = "INSERT INTO upload_sessions (id, account_id, blob_id, client_key, declared_size, \
     block_count, reserved_bytes, received, block_hashes, created_at, last_checkpoint_at, expires_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $5, $7, $8, \
     'epoch'::timestamptz + $9::bigint * interval '1 microsecond', \
     'epoch'::timestamptz + $9::bigint * interval '1 microsecond', \
     'epoch'::timestamptz + $10::bigint * interval '1 microsecond')";

const DELETE_OPEN: &str = "DELETE FROM upload_sessions \
     WHERE id = $1 AND account_id = $2 AND committed_revision_id IS NULL";

// A checkpoint rewrites the progress columns and rolls the expiry; a committed or already expired row is never
// written (the reaper owns an expired one).
const CHECKPOINT: &str = "UPDATE upload_sessions SET received = $3, block_hashes = $4, \
     last_checkpoint_at = 'epoch'::timestamptz + $5::bigint * interval '1 microsecond', \
     expires_at = 'epoch'::timestamptz + $6::bigint * interval '1 microsecond' \
     WHERE id = $1 AND account_id = $2 AND committed_revision_id IS NULL \
     AND expires_at > 'epoch'::timestamptz + $5::bigint * interval '1 microsecond'";

/// The row a `client_key` names. `committed_revision_id` is `Some` once its commit created that revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyRow {
    pub id: Uuid,
    pub declared_size: i64,
    pub block_count: i32,
    pub received: Vec<u8>,
    pub expires_at: Timestamp,
    pub committed_revision_id: Option<Uuid>,
}

/// A new open session; `reserved_bytes` equals `declared_size` and nothing is received.
pub(crate) struct NewSession<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub blob_id: Uuid,
    pub client_key: &'a str,
    pub declared_size: i64,
    pub block_count: i32,
    pub now: Timestamp,
    pub expires_at: Timestamp,
}

pub(crate) async fn by_key(
    tx: &mut DbTx,
    account: Uuid,
    client_key: &str,
) -> Result<Option<KeyRow>, DbError> {
    let row = sqlx::query(BY_KEY)
        .bind(account)
        .bind(client_key)
        .fetch_optional(tx.conn())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(KeyRow {
        id: row.try_get(0)?,
        declared_size: row.try_get(1)?,
        block_count: row.try_get(2)?,
        received: row.try_get(3)?,
        expires_at: Timestamp::from_unix_micros(row.try_get(4)?),
        committed_revision_id: row.try_get(5)?,
    }))
}

/// Removes the key's committed row once it is past `expires_at`, as the reaper would (its blob is the revision's
/// and stays). Returns the rows removed, 0 or 1.
pub(crate) async fn delete_expired_committed(
    tx: &mut DbTx,
    account: Uuid,
    client_key: &str,
    now: Timestamp,
) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_EXPIRED_COMMITTED)
        .bind(account)
        .bind(client_key)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

pub(crate) async fn count_open_large(tx: &mut DbTx, account: Uuid) -> Result<i64, DbError> {
    Ok(sqlx::query_scalar(COUNT_OPEN_LARGE)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

pub(crate) async fn insert(tx: &mut DbTx, session: &NewSession<'_>) -> Result<(), DbError> {
    let blocks = usize::try_from(session.block_count).unwrap_or(0);
    sqlx::query(INSERT)
        .bind(session.id)
        .bind(session.account)
        .bind(session.blob_id)
        .bind(session.client_key)
        .bind(session.declared_size)
        .bind(session.block_count)
        .bind(vec![0u8; blocks.div_ceil(8)])
        .bind(vec![0u8; blocks * 32])
        .bind(session.now.as_unix_micros())
        .bind(session.expires_at.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// Deletes an open session row; returns the rows removed, 0 or 1.
pub(crate) async fn delete_open(tx: &mut DbTx, account: Uuid, id: Uuid) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_OPEN)
        .bind(id)
        .bind(account)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// The progress columns of an open session as a checkpoint writes them.
pub(crate) struct Progress<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub received: &'a [u8],
    pub block_hashes: &'a [u8],
    pub now: Timestamp,
    pub expires_at: Timestamp,
}

/// Writes a checkpoint; returns the rows written, 0 when the session is gone, committed or expired.
pub(crate) async fn checkpoint(tx: &mut DbTx, progress: &Progress<'_>) -> Result<u64, DbError> {
    let done = sqlx::query(CHECKPOINT)
        .bind(progress.id)
        .bind(progress.account)
        .bind(progress.received)
        .bind(progress.block_hashes)
        .bind(progress.now.as_unix_micros())
        .bind(progress.expires_at.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}
