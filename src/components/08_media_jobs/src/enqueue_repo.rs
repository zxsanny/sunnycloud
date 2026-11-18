//! The job statements of the enqueue side, run on the caller's account-scoped transaction (the commit that plans
//! the jobs).

use media_shared::policy::JobSpec;
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use uuid::Uuid;

use crate::job_names::kind_name;
use crate::job_repo::{at, micros, overflow};

/// A job of the same key whose revision was collected (`revision_id` NULL) would otherwise keep the new revision
/// of the same content from ever being processed: it is adopted by the new revision and queued afresh, whatever
/// its status. A job that still has its revision is left as it is.
const ENQUEUE: &str = concat!(
    "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
     available_at, created_at) VALUES ($1, $2, $3, $4, $5, $6::bigint, $7, 'queued', ",
    at!("$8"),
    ", ",
    at!("$8"),
    ") ON CONFLICT ON CONSTRAINT jobs_key DO UPDATE SET revision_id = EXCLUDED.revision_id, \
     priority = EXCLUDED.priority, status = 'queued', attempts = 0, available_at = EXCLUDED.available_at, \
     leased_by = NULL, outcome = NULL, last_error = NULL, created_at = EXCLUDED.created_at, finished_at = NULL \
     WHERE jobs.revision_id IS NULL"
);

/// The account's queued jobs, at most `$2`, on `jobs_queued_idx`.
const COUNT_QUEUED: &str = "SELECT count(*) FROM (SELECT 1 FROM jobs WHERE account_id = $1 AND status = 'queued' \
     LIMIT $2::bigint) AS queued";

pub(crate) async fn enqueue(
    tx: &mut DbTx,
    id: Uuid,
    spec: &JobSpec,
    priority: i16,
    now: Timestamp,
) -> Result<bool, DbError> {
    let done = sqlx::query(ENQUEUE)
        .bind(id)
        .bind(spec.account_id)
        .bind(spec.revision_id)
        .bind(kind_name(spec.kind))
        .bind(spec.content_hash.as_bytes().as_slice())
        .bind(i64::from(spec.params_version))
        .bind(priority)
        .bind(micros(now))
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

/// The number of queued jobs of `account`, counted up to `limit`.
pub(crate) async fn count_queued(tx: &mut DbTx, account: Uuid, limit: u64) -> Result<u64, DbError> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let count: i64 = sqlx::query_scalar(COUNT_QUEUED)
        .bind(account)
        .bind(limit)
        .fetch_one(tx.conn())
        .await?;
    u64::try_from(count).map_err(|_| overflow("queued count"))
}
