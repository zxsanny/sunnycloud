//! The statements of the job queue. Enqueue runs on the caller's account-scoped transaction; every other
//! operation runs in system scope in one short transaction of its own, because a claim has no account yet
//! and the reaper, the purge and the requeue cross accounts.
//!
//! Times are bound as microseconds of the API clock (`scripts/ci/check-no-sql-now.sh`). A lease is live while
//! `available_at > now`; the instant of expiry itself is already expired and claimable.

use std::time::Duration;

use media_shared::policy::JobSpec;
use media_shared::types::{ClaimedJob, Disposition, JobKind, Lane, SkipOutcome};
use sc_clock::Timestamp;
use sc_db::{Db, DbError, DbTx, RunError};
use sc_ids::FileHash;
use sqlx::AssertSqlSafe;
use uuid::Uuid;

use crate::retry;

macro_rules! at {
    ($param:literal) => {
        concat!(
            "('epoch'::timestamptz + ",
            $param,
            "::bigint * interval '1 microsecond')"
        )
    };
}

const ENQUEUE: &str = concat!(
    "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
     available_at, created_at) VALUES ($1, $2, $3, $4, $5, $6::bigint, $7, 'queued', ",
    at!("$8"),
    ", ",
    at!("$8"),
    ") ON CONFLICT ON CONSTRAINT jobs_key DO NOTHING"
);

/// The account's queued jobs, at most `$2`, on `jobs_queued_idx`.
const COUNT_QUEUED: &str = "SELECT count(*) FROM (SELECT 1 FROM jobs WHERE account_id = $1 AND status = 'queued' \
     LIMIT $2::bigint) AS queued";

/// One statement: the next candidate of the lane in claim-index order is locked with `SKIP LOCKED`, and the
/// update either leases it or, when its attempts are exhausted (an expired lease of the last attempt), marks it
/// `dead` in place. `exhausted` and `was` tell the caller which happened.
const CLAIM: &str = concat!(
    "WITH next AS MATERIALIZED (SELECT id, status AS was, attempts >= $3::bigint AS exhausted FROM jobs \
     WHERE lane = $1 AND status IN ('queued', 'running') AND available_at <= ",
    at!("$2"),
    " ORDER BY priority, available_at FOR UPDATE SKIP LOCKED LIMIT 1) \
     UPDATE jobs SET \
     status = CASE WHEN next.exhausted THEN 'dead' ELSE 'running' END, \
     attempts = jobs.attempts + CASE WHEN next.exhausted THEN 0 ELSE 1 END, \
     available_at = CASE WHEN next.exhausted THEN jobs.available_at ELSE ",
    at!("$5"),
    " END, \
     leased_by = CASE WHEN next.exhausted THEN jobs.leased_by ELSE $4 END, \
     finished_at = CASE WHEN next.exhausted THEN ",
    at!("$2"),
    " END, \
     last_error = CASE WHEN next.exhausted THEN COALESCE(jobs.last_error, 'lease_expired') ELSE jobs.last_error END \
     FROM next WHERE jobs.id = next.id \
     RETURNING jobs.id, jobs.kind, jobs.content_hash, jobs.params_version, jobs.attempts, next.exhausted, \
     next.was = 'running'"
);

const EXTEND: &str = concat!(
    "UPDATE jobs SET available_at = ",
    at!("$4"),
    " ",
    "WHERE id = $1 AND status = 'running' AND leased_by = $2 AND available_at > ",
    at!("$3")
);

const RELEASE: &str = concat!(
    "UPDATE jobs SET status = 'queued', attempts = attempts - 1, available_at = ",
    at!("$3"),
    ", leased_by = NULL WHERE id = $1 AND status = 'running' AND leased_by = $2 AND available_at > ",
    at!("$3")
);

const SKIP: &str = concat!(
    "UPDATE jobs SET status = 'succeeded', outcome = $4, finished_at = ",
    at!("$3"),
    " WHERE id = $1 AND status = 'running' AND leased_by = $2 AND available_at > ",
    at!("$3")
);

const LOCK_HOLDER: &str = concat!(
    "SELECT attempts FROM jobs WHERE id = $1 AND status = 'running' AND leased_by = $2 AND available_at > ",
    at!("$3"),
    " FOR UPDATE"
);

const MARK_DEAD: &str = concat!(
    "UPDATE jobs SET status = 'dead', finished_at = ",
    at!("$2"),
    ", last_error = $3 WHERE id = $1"
);

const REQUEUE_FAILED: &str = concat!(
    "UPDATE jobs SET status = 'queued', available_at = ",
    at!("$2"),
    ", leased_by = NULL, last_error = $3 WHERE id = $1"
);

const REAP: &str = concat!(
    "UPDATE jobs SET status = 'dead', finished_at = ",
    at!("$1"),
    ", last_error = COALESCE(last_error, 'lease_expired') \
     WHERE status = 'running' AND attempts >= $2::bigint AND available_at <= ",
    at!("$1")
);

const PURGE: &str = concat!(
    "WITH batch AS MATERIALIZED (SELECT id FROM jobs WHERE status IN ('succeeded', 'dead') AND finished_at < ",
    at!("$1"),
    " ORDER BY finished_at LIMIT $2 FOR UPDATE SKIP LOCKED) \
     DELETE FROM jobs USING batch WHERE jobs.id = batch.id"
);

const REQUEUE_DEAD: &str = concat!(
    "UPDATE jobs SET status = 'queued', attempts = 0, available_at = ",
    at!("$1"),
    ", leased_by = NULL, finished_at = NULL, outcome = NULL \
     WHERE status = 'dead' AND ($2::uuid IS NULL OR account_id = $2) AND ($3::text IS NULL OR kind = $3)"
);

pub(crate) const PURGE_BATCH: i64 = 1_000;

pub(crate) fn kind_name(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Stills => "stills",
        JobKind::Poster => "poster",
        JobKind::Transcode => "transcode",
    }
}

pub(crate) fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::Image => "image",
        Lane::Video => "video",
    }
}

fn parse_kind(name: &str) -> Result<JobKind, DbError> {
    match name {
        "stills" => Ok(JobKind::Stills),
        "poster" => Ok(JobKind::Poster),
        "transcode" => Ok(JobKind::Transcode),
        other => Err(DbError::Other(format!("unknown job kind {other:?}"))),
    }
}

fn skip_name(outcome: SkipOutcome) -> &'static str {
    match outcome {
        SkipOutcome::SkippedCompliant => "skipped_compliant",
        SkipOutcome::Obsolete => "obsolete",
    }
}

fn micros(at: Timestamp) -> i64 {
    at.as_unix_micros()
}

fn overflow(what: &str) -> DbError {
    DbError::Other(format!("stored {what} is out of range"))
}

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

/// id, kind, content hash, params_version, attempts, exhausted, was running.
type ClaimRow = (Uuid, String, Vec<u8>, i16, i16, bool, bool);

pub(crate) enum Next {
    Empty,
    /// The candidate's attempts were used up: it was marked dead, nothing is leased.
    Exhausted {
        was_running: bool,
    },
    Leased {
        job: ClaimedJob,
        took_over: bool,
    },
}

pub(crate) struct Lease<'a> {
    pub worker: &'a str,
    pub now: Timestamp,
    pub until: Timestamp,
    pub lease_ms: u64,
    pub max_attempts: i64,
}

pub(crate) async fn claim(
    db: &Db,
    lane: Lane,
    lease: &Lease<'_>,
) -> Result<Next, RunError<DbError>> {
    db.run_system("job_claim", async |tx| {
        let row: Option<ClaimRow> = sqlx::query_as(CLAIM)
            .bind(lane_name(lane))
            .bind(micros(lease.now))
            .bind(lease.max_attempts)
            .bind(lease.worker)
            .bind(micros(lease.until))
            .fetch_optional(tx.conn())
            .await?;
        let Some((id, kind, hash, params_version, attempts, exhausted, was_running)) = row else {
            return Ok(Next::Empty);
        };
        if exhausted {
            return Ok(Next::Exhausted { was_running });
        }
        let hash = <[u8; 32]>::try_from(hash).map_err(|_| overflow("content hash"))?;
        Ok::<_, DbError>(Next::Leased {
            job: ClaimedJob {
                job_id: id,
                kind: parse_kind(&kind)?,
                content_hash: FileHash::from_bytes(hash),
                params_version: u32::try_from(params_version)
                    .map_err(|_| overflow("params_version"))?,
                attempt: u32::try_from(attempts).map_err(|_| overflow("attempts"))?,
                lease_ms: lease.lease_ms,
            },
            took_over: was_running,
        })
    })
    .await
}

pub(crate) async fn extend(
    db: &Db,
    job: Uuid,
    worker: &str,
    now: Timestamp,
    until: Timestamp,
) -> Result<bool, RunError<DbError>> {
    db.run_system("job_extend_lease", async |tx| {
        let done = sqlx::query(EXTEND)
            .bind(job)
            .bind(worker)
            .bind(micros(now))
            .bind(micros(until))
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(done.rows_affected() == 1)
    })
    .await
}

pub(crate) async fn release(
    db: &Db,
    job: Uuid,
    worker: &str,
    now: Timestamp,
) -> Result<bool, RunError<DbError>> {
    db.run_system("job_release", async |tx| {
        let done = sqlx::query(RELEASE)
            .bind(job)
            .bind(worker)
            .bind(micros(now))
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(done.rows_affected() == 1)
    })
    .await
}

pub(crate) async fn complete_skipped(
    db: &Db,
    job: Uuid,
    worker: &str,
    now: Timestamp,
    outcome: SkipOutcome,
) -> Result<bool, RunError<DbError>> {
    db.run_system("job_complete_skipped", async |tx| {
        let done = sqlx::query(SKIP)
            .bind(job)
            .bind(worker)
            .bind(micros(now))
            .bind(skip_name(outcome))
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(done.rows_affected() == 1)
    })
    .await
}

pub(crate) enum Failed {
    LeaseLost,
    Dead,
    Requeued,
}

pub(crate) struct FailPlan<'a> {
    pub worker: &'a str,
    pub now: Timestamp,
    pub disposition: Disposition,
    pub max_attempts: i64,
    pub backoff: Duration,
    pub error: &'a str,
}

/// The holder row is locked and read, then closed `dead` or put back to `queued` with its backoff, in one
/// transaction.
pub(crate) async fn fail(
    db: &Db,
    job: Uuid,
    plan: &FailPlan<'_>,
) -> Result<Failed, RunError<DbError>> {
    db.run_system("job_fail", async |tx| {
        let attempts: Option<i16> = sqlx::query_scalar(LOCK_HOLDER)
            .bind(job)
            .bind(plan.worker)
            .bind(micros(plan.now))
            .fetch_optional(tx.conn())
            .await?;
        let Some(attempts) = attempts else {
            return Ok(Failed::LeaseLost);
        };
        let attempts = u32::try_from(attempts).map_err(|_| overflow("attempts"))?;
        let dead = retry::is_dead(plan.disposition, i64::from(attempts), plan.max_attempts);
        if dead {
            sqlx::query(MARK_DEAD)
                .bind(job)
                .bind(micros(plan.now))
                .bind(plan.error)
                .execute(tx.conn())
                .await?;
            return Ok(Failed::Dead);
        }
        let retry_at = retry::add(plan.now, retry::retry_delay(plan.backoff, attempts));
        sqlx::query(REQUEUE_FAILED)
            .bind(job)
            .bind(micros(retry_at))
            .bind(plan.error)
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(Failed::Requeued)
    })
    .await
}

pub(crate) async fn reap(
    db: &Db,
    now: Timestamp,
    max_attempts: i64,
) -> Result<u64, RunError<DbError>> {
    db.run_system("job_reap", async |tx| {
        let done = sqlx::query(REAP)
            .bind(micros(now))
            .bind(max_attempts)
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(done.rows_affected())
    })
    .await
}

pub(crate) async fn purge_batch(db: &Db, before: Timestamp) -> Result<u64, RunError<DbError>> {
    db.run_system("job_purge", async |tx| {
        let done = sqlx::query(PURGE)
            .bind(micros(before))
            .bind(PURGE_BATCH)
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(done.rows_affected())
    })
    .await
}

pub(crate) async fn requeue_dead(
    db: &Db,
    now: Timestamp,
    account: Option<Uuid>,
    kind: Option<JobKind>,
) -> Result<u64, RunError<DbError>> {
    db.run_system("job_requeue_dead", async |tx| {
        let done = sqlx::query(REQUEUE_DEAD)
            .bind(micros(now))
            .bind(account)
            .bind(kind.map(kind_name))
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(done.rows_affected())
    })
    .await
}

/// `EXPLAIN` of the claim as the queue runs it, for the plan test.
#[doc(hidden)]
pub async fn explain_claim(
    db: &Db,
    lane: Lane,
    now: Timestamp,
    max_attempts: i64,
) -> Result<String, RunError<DbError>> {
    db.run_system("job_claim_plan", async |tx| {
        let lines: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN {CLAIM}")))
            .bind(lane_name(lane))
            .bind(micros(now))
            .bind(max_attempts)
            .bind("plan-probe")
            .bind(micros(now))
            .fetch_all(tx.conn())
            .await?;
        Ok::<_, DbError>(lines.join("\n"))
    })
    .await
}
