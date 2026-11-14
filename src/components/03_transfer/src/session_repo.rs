//! The `upload_sessions` statements of `start_upload`, the `put_block` checkpoint, the lazy load,
//! `abort_upload`, `stage_small`, the commit effects and the staged rows of `commit_batch`. Every statement names the account, so a row of another account is never read or written even
//! in system scope. Timestamps travel as Unix microseconds.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::Hash32;
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

// Staged rows: open, at most one block and every block received (the complement of COUNT_OPEN_LARGE among open
// rows; see there for the one-block large session counted here until it commits).
const COUNT_STAGED: &str = "SELECT count(*) FROM upload_sessions \
     WHERE account_id = $1 AND committed_revision_id IS NULL \
     AND block_count <= 1 AND received <> '\\x00'::bytea";

// What a repeated `stage_small` compares: the first block hash only (a staged row has at most one), so a large
// session's hash array is never read in full.
const STAGED_BY_KEY: &str = "SELECT declared_size, block_count, received, \
     substring(block_hashes FROM 1 FOR 32), committed_revision_id IS NOT NULL \
     FROM upload_sessions WHERE account_id = $1 AND client_key = $2";

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

// An open session that has not expired: what the progress entry is rebuilt from after a restart.
const OPEN_SESSION: &str = "SELECT blob_id, declared_size, block_count, received, block_hashes, \
     (extract(epoch FROM expires_at) * 1000000)::bigint \
     FROM upload_sessions WHERE id = $1 AND account_id = $2 AND committed_revision_id IS NULL \
     AND expires_at > 'epoch'::timestamptz + $3::bigint * interval '1 microsecond'";

// Abort: an open, unexpired row only (an expired one is the reaper's); the deleter settles the reservation.
const DELETE_OPEN_RETURNING: &str = "DELETE FROM upload_sessions \
     WHERE id = $1 AND account_id = $2 AND committed_revision_id IS NULL \
     AND expires_at > 'epoch'::timestamptz + $3::bigint * interval '1 microsecond' \
     RETURNING blob_id, reserved_bytes";

// `commit_batch`: the open, unexpired staged rows (see COUNT_STAGED) a batch names by key.
const STAGED_BY_KEYS: &str = "SELECT id, client_key, blob_id, declared_size, reserved_bytes, \
     substring(block_hashes FROM 1 FOR 32) \
     FROM upload_sessions WHERE account_id = $1 AND client_key = ANY($2) \
     AND committed_revision_id IS NULL AND block_count <= 1 AND received <> '\\x00'::bytea \
     AND expires_at > 'epoch'::timestamptz + $3::bigint * interval '1 microsecond'";

// The commit effect's lock: an open row only, so an aborted, reaped or already committed session gives no row.
const LOCK_OPEN: &str = "SELECT 1 FROM upload_sessions \
     WHERE id = $1 AND account_id = $2 AND committed_revision_id IS NULL FOR UPDATE";

// A large session's row becomes the committed-key record (owner decision 2026-11-13): no reservation, kept until
// commit time + `upload_session_ttl`.
const MARK_COMMITTED: &str = "UPDATE upload_sessions SET committed_revision_id = $3, reserved_bytes = 0, \
     expires_at = 'epoch'::timestamptz + $4::bigint * interval '1 microsecond' \
     WHERE id = $1 AND account_id = $2 AND committed_revision_id IS NULL";

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

/// A new session row; `reserved_bytes` equals `declared_size`. [`insert`] writes it with nothing received,
/// [`insert_staged`] with its only block (if any) received.
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

pub(crate) async fn count_staged(tx: &mut DbTx, account: Uuid) -> Result<i64, DbError> {
    Ok(sqlx::query_scalar(COUNT_STAGED)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

/// The row a `staged_key` names as a repeated `stage_small` compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedKeyRow {
    pub declared_size: i64,
    pub block_count: i32,
    pub received: Vec<u8>,
    /// The first 32 bytes of `block_hashes`; empty for a row without blocks.
    pub first_hash: Vec<u8>,
    pub committed: bool,
}

pub(crate) async fn staged_by_key(
    tx: &mut DbTx,
    account: Uuid,
    client_key: &str,
) -> Result<Option<StagedKeyRow>, DbError> {
    let row = sqlx::query(STAGED_BY_KEY)
        .bind(account)
        .bind(client_key)
        .fetch_optional(tx.conn())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(StagedKeyRow {
        declared_size: row.try_get(0)?,
        block_count: row.try_get(1)?,
        received: row.try_get(2)?,
        first_hash: row.try_get(3)?,
        committed: row.try_get(4)?,
    }))
}

/// A staged row as `commit_batch` publishes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedRow {
    pub id: Uuid,
    pub client_key: String,
    pub blob_id: Uuid,
    pub declared_size: i64,
    pub reserved_bytes: i64,
    /// The hash of its only block; empty for a zero-byte file.
    pub first_hash: Vec<u8>,
}

/// The open staged rows among `keys` that have not expired at `now`.
pub(crate) async fn staged_by_keys(
    tx: &mut DbTx,
    account: Uuid,
    keys: &[String],
    now: Timestamp,
) -> Result<Vec<StagedRow>, DbError> {
    let rows = sqlx::query(STAGED_BY_KEYS)
        .bind(account)
        .bind(keys)
        .bind(now.as_unix_micros())
        .fetch_all(tx.conn())
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StagedRow {
                id: row.try_get(0)?,
                client_key: row.try_get(1)?,
                blob_id: row.try_get(2)?,
                declared_size: row.try_get(3)?,
                reserved_bytes: row.try_get(4)?,
                first_hash: row.try_get(5)?,
            })
        })
        .collect()
}

pub(crate) async fn count_open_large(tx: &mut DbTx, account: Uuid) -> Result<i64, DbError> {
    Ok(sqlx::query_scalar(COUNT_OPEN_LARGE)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

pub(crate) async fn insert(tx: &mut DbTx, session: &NewSession<'_>) -> Result<(), DbError> {
    let blocks = usize::try_from(session.block_count).unwrap_or(0);
    insert_row(
        tx,
        session,
        &vec![0u8; blocks.div_ceil(8)],
        &vec![0u8; blocks * 32],
    )
    .await
}

/// A staged row: `hash` is the hash of its only block, `None` for a zero-byte file (no block).
pub(crate) async fn insert_staged(
    tx: &mut DbTx,
    session: &NewSession<'_>,
    hash: Option<&Hash32>,
) -> Result<(), DbError> {
    match hash {
        Some(hash) => insert_row(tx, session, &[1], hash.as_bytes()).await,
        None => insert_row(tx, session, &[], &[]).await,
    }
}

async fn insert_row(
    tx: &mut DbTx,
    session: &NewSession<'_>,
    received: &[u8],
    block_hashes: &[u8],
) -> Result<(), DbError> {
    sqlx::query(INSERT)
        .bind(session.id)
        .bind(session.account)
        .bind(session.blob_id)
        .bind(session.client_key)
        .bind(session.declared_size)
        .bind(session.block_count)
        .bind(received)
        .bind(block_hashes)
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

/// An open session row as the lazy load reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenRow {
    pub blob_id: Uuid,
    pub declared_size: i64,
    pub block_count: i32,
    pub received: Vec<u8>,
    pub block_hashes: Vec<u8>,
    pub expires_at: Timestamp,
}

/// The open, unexpired session `id` of `account`; `None` when absent, foreign, committed or expired at `now`.
pub(crate) async fn open_session(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    now: Timestamp,
) -> Result<Option<OpenRow>, DbError> {
    let row = sqlx::query(OPEN_SESSION)
        .bind(id)
        .bind(account)
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(OpenRow {
        blob_id: row.try_get(0)?,
        declared_size: row.try_get(1)?,
        block_count: row.try_get(2)?,
        received: row.try_get(3)?,
        block_hashes: row.try_get(4)?,
        expires_at: Timestamp::from_unix_micros(row.try_get(5)?),
    }))
}

/// Deletes the open, unexpired session `id`; returns its `(blob_id, reserved_bytes)`, `None` when no row went.
pub(crate) async fn delete_open_returning(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    now: Timestamp,
) -> Result<Option<(Uuid, i64)>, DbError> {
    Ok(sqlx::query_as(DELETE_OPEN_RETURNING)
        .bind(id)
        .bind(account)
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?)
}

/// Locks the open session `id` for the rest of the transaction; `false` when there is no open row.
pub(crate) async fn lock_open(tx: &mut DbTx, account: Uuid, id: Uuid) -> Result<bool, DbError> {
    let row: Option<i32> = sqlx::query_scalar(LOCK_OPEN)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.is_some())
}

/// Turns the open session `id` into the committed-key record of `revision`; returns the rows written, 0 or 1.
pub(crate) async fn mark_committed(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    revision: Uuid,
    expires_at: Timestamp,
) -> Result<u64, DbError> {
    let done = sqlx::query(MARK_COMMITTED)
        .bind(id)
        .bind(account)
        .bind(revision)
        .bind(expires_at.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}
