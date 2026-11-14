//! The statements of the media maintenance ports: the account erase hook (on its account-scoped transaction), the
//! blob owner cleanup (on the GC sweeper's transaction), the derived janitor and the stale worker rows (on the
//! system-scope transactions `job_repo` opens).
//!
//! Lock order: a derived row is locked first (`FOR UPDATE SKIP LOCKED`), then its blob row through the GC queue,
//! in the erase and in the janitor alike. The GC sweeper locks a blob row and then deletes its derived row, but it
//! only locks queued blobs, and a derived blob is queued in the same transaction that deletes its row, so the
//! sweeper never waits on a row these statements hold.

use contracts::{Due, Fault, GcQueue, GcReason};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use uuid::Uuid;

use crate::job_repo::{at, micros, overflow};

/// Up to `$2` of the account's jobs, whatever their status. A job locked by a claim or a report is skipped and
/// stays in the remaining count until a later call.
const DELETE_ACCOUNT_JOBS: &str = "WITH picked AS MATERIALIZED (SELECT id FROM jobs WHERE account_id = $1 \
     ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED) DELETE FROM jobs USING picked WHERE jobs.id = picked.id";

const LOCK_ACCOUNT_DERIVED: &str = "SELECT blob_id FROM derived WHERE account_id = $1 ORDER BY blob_id \
     LIMIT $2 FOR UPDATE SKIP LOCKED";

/// Derived rows whose (account, content hash) has no revision left, on `revisions_account_id_content_hash_idx`.
const LOCK_ORPHANS: &str = "SELECT d.blob_id FROM derived d WHERE NOT EXISTS (SELECT 1 FROM revisions r \
     WHERE r.account_id = d.account_id AND r.content_hash = d.content_hash) ORDER BY d.blob_id LIMIT $1 \
     FOR UPDATE OF d SKIP LOCKED";

/// `blob_id` is unique across accounts (`derived_blob_id_key`).
const DELETE_DERIVED: &str = "DELETE FROM derived WHERE blob_id = ANY($1)";

const REMAINING: &str = "SELECT (SELECT count(*) FROM jobs WHERE account_id = $1), \
     (SELECT count(*) FROM derived WHERE account_id = $1)";

const PURGE_WORKER_STATUS: &str = concat!("DELETE FROM worker_status WHERE seen_at < ", at!("$1"));

pub(crate) async fn delete_account_jobs(
    tx: &mut DbTx,
    account: Uuid,
    limit: u64,
) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_ACCOUNT_JOBS)
        .bind(account)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// The blobs of up to `limit` of the account's derived rows, the rows locked.
pub(crate) async fn lock_account_derived(
    tx: &mut DbTx,
    account: Uuid,
    limit: u64,
) -> Result<Vec<Uuid>, DbError> {
    Ok(sqlx::query_scalar(LOCK_ACCOUNT_DERIVED)
        .bind(account)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(tx.conn())
        .await?)
}

/// The blobs of up to `limit` orphan derived rows of any account, the rows locked. System scope only.
pub(crate) async fn lock_orphans(tx: &mut DbTx, limit: u32) -> Result<Vec<Uuid>, DbError> {
    Ok(sqlx::query_scalar(LOCK_ORPHANS)
        .bind(i64::from(limit))
        .fetch_all(tx.conn())
        .await?)
}

/// Each blob queued `Due::Now` with `reason`, then the derived rows of the blobs deleted; returns the rows deleted.
/// Derived blobs have no retention.
pub(crate) async fn queue_and_delete(
    tx: &mut DbTx,
    gc: &dyn GcQueue,
    blobs: &[Uuid],
    reason: GcReason,
) -> Result<u64, Fault> {
    for &blob in blobs {
        gc.schedule_in_tx(tx, blob, reason, Due::Now).await?;
    }
    Ok(delete_derived(tx, blobs).await?)
}

pub(crate) async fn delete_derived(tx: &mut DbTx, blobs: &[Uuid]) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_DERIVED)
        .bind(blobs)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// The account's jobs plus its derived rows.
pub(crate) async fn remaining(tx: &mut DbTx, account: Uuid) -> Result<u64, DbError> {
    let (jobs, derived): (i64, i64) = sqlx::query_as(REMAINING)
        .bind(account)
        .fetch_one(tx.conn())
        .await?;
    let jobs = u64::try_from(jobs).map_err(|_| overflow("job count"))?;
    let derived = u64::try_from(derived).map_err(|_| overflow("derived count"))?;
    Ok(jobs.saturating_add(derived))
}

pub(crate) async fn purge_worker_status(tx: &mut DbTx, before: Timestamp) -> Result<u64, DbError> {
    let done = sqlx::query(PURGE_WORKER_STATUS)
        .bind(micros(before))
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}
