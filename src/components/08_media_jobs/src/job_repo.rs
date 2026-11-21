//! The system-scope transactions of 08 and the statements of the job queue they run. Enqueue runs on the
//! caller's account-scoped transaction (`enqueue_repo`); every operation here runs in system scope in one short
//! transaction of its own, because a claim has no account yet and the reaper, the purges, the derived janitor,
//! the requeue, the queue counts and the worker status cross accounts. Each is the only `run_system` call site of
//! its operation (`scripts/ci/run-system-allowlist.txt`); statements of other files run on the transaction it opens.
//!
//! Times are bound as microseconds of the API clock (`scripts/ci/check-no-sql-now.sh`). A lease is live while
//! `available_at > now`; the instant of expiry itself is already expired and claimable.

use contracts::{Fault, GcQueue, GcReason};
use media_shared::types::{ClaimedJob, JobKind, Lane, SkipOutcome};
use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError};
use sc_ids::FileHash;
use sqlx::AssertSqlSafe;
use uuid::Uuid;

use crate::admin_repo::{self, JobCountRow, WorkerFacts};
use crate::job_fail_repo::{self, FailPlan, Failed};
use crate::job_names::{kind_name, lane_name, parse_kind, skip_name};
use crate::maintenance_repo;

macro_rules! at {
    ($param:literal) => {
        concat!(
            "('epoch'::timestamptz + ",
            $param,
            "::bigint * interval '1 microsecond')"
        )
    };
}
pub(crate) use at;

/// The `RETURNING` columns of a job that just became terminal, read by [`closed`].
macro_rules! closed_columns {
    () => {
        "jobs.kind, extract(epoch FROM jobs.finished_at - jobs.created_at)::float8, jobs.account_id, \
         jobs.content_hash"
    };
}
pub(crate) use closed_columns;

/// A job that just became `succeeded` or `dead`: its kind, the seconds from its enqueue to its close, and the
/// account and content it was for (the realtime channel announces a failed preview by them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Closed {
    pub kind: JobKind,
    pub seconds: f64,
    pub account: Uuid,
    pub content_hash: FileHash,
}

/// The row `closed_columns!` returns: kind, seconds, account, content hash.
pub(crate) type ClosedRow = (String, f64, Uuid, Vec<u8>);

pub(crate) fn closed((kind, seconds, account, hash): ClosedRow) -> Result<Closed, DbError> {
    let hash = <[u8; 32]>::try_from(hash).map_err(|_| overflow("content hash"))?;
    Ok(Closed {
        kind: parse_kind(&kind)?,
        seconds,
        account,
        content_hash: FileHash::from_bytes(hash),
    })
}

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
     next.was = 'running', extract(epoch FROM jobs.finished_at - jobs.created_at)::float8, jobs.account_id"
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
    at!("$3"),
    " RETURNING ",
    closed_columns!()
);

/// The job running under the worker's live lease; with `$4`, also one closed `derived` while it held it (an output
/// post retried after a lost answer).
const HELD: &str = concat!(
    "SELECT account_id, revision_id, kind, content_hash, params_version FROM jobs \
     WHERE id = $1 AND leased_by = $2 AND ((status = 'running' AND available_at > ",
    at!("$3"),
    ") OR ($4 AND status = 'succeeded' AND outcome = 'derived'))"
);

const REAP: &str = concat!(
    "UPDATE jobs SET status = 'dead', finished_at = ",
    at!("$1"),
    ", last_error = COALESCE(last_error, 'lease_expired') \
     WHERE status = 'running' AND attempts >= $2::bigint AND available_at <= ",
    at!("$1"),
    " RETURNING ",
    closed_columns!()
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

/// One row per worker id, replaced by each report; `seen_at` is the API clock's.
const RECORD_STATUS: &str = concat!(
    "INSERT INTO worker_status (worker_id, seen_at, image_age_days, decoder_ok) VALUES ($1, ",
    at!("$2"),
    ", $3, $4) ON CONFLICT ON CONSTRAINT worker_status_pkey DO UPDATE SET seen_at = EXCLUDED.seen_at, \
     image_age_days = EXCLUDED.image_age_days, decoder_ok = EXCLUDED.decoder_ok"
);

pub(crate) const PURGE_BATCH: i64 = 1_000;

pub(crate) fn micros(at: Timestamp) -> i64 {
    at.as_unix_micros()
}

pub(crate) fn overflow(what: &str) -> DbError {
    DbError::Other(format!("stored {what} is out of range"))
}

/// id, kind, content hash, params_version, attempts, exhausted, was running, seconds since enqueue when closed.
type ClaimRow = (
    Uuid,
    String,
    Vec<u8>,
    i16,
    i16,
    bool,
    bool,
    Option<f64>,
    Uuid,
);

pub(crate) enum Next {
    Empty,
    /// The candidate's attempts were used up: it was marked dead, nothing is leased.
    Exhausted {
        was_running: bool,
        closed: Closed,
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
        let Some((
            id,
            kind,
            hash,
            params_version,
            attempts,
            exhausted,
            was_running,
            seconds,
            account,
        )) = row
        else {
            return Ok(Next::Empty);
        };
        if exhausted {
            let seconds = seconds.ok_or_else(|| overflow("finished_at"))?;
            return Ok(Next::Exhausted {
                was_running,
                closed: closed((kind, seconds, account, hash))?,
            });
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
) -> Result<Option<Closed>, RunError<DbError>> {
    db.run_system("job_complete_skipped", async |tx| {
        let row: Option<ClosedRow> = sqlx::query_as(SKIP)
            .bind(job)
            .bind(worker)
            .bind(micros(now))
            .bind(skip_name(outcome))
            .fetch_optional(tx.conn())
            .await?;
        row.map(closed).transpose()
    })
    .await
}

/// A held job as `accept_output` and `open_input` need it.
pub(crate) struct HeldJob {
    pub account: Uuid,
    /// `None` once the revision is collected (`ON DELETE SET NULL`): the job is obsolete.
    pub revision: Option<Uuid>,
    pub kind: JobKind,
    pub content_hash: FileHash,
    pub params_version: u32,
}

/// account, revision, kind, content hash, params_version.
type HeldRow = (Uuid, Option<Uuid>, String, Vec<u8>, i16);

/// System scope: the job's account is not known before this lookup.
pub(crate) async fn held(
    db: &Db,
    job: Uuid,
    worker: &str,
    now: Timestamp,
    closed_derived_ok: bool,
) -> Result<Option<HeldJob>, RunError<DbError>> {
    db.run_system("job_held", async |tx| {
        let row: Option<HeldRow> = sqlx::query_as(HELD)
            .bind(job)
            .bind(worker)
            .bind(micros(now))
            .bind(closed_derived_ok)
            .fetch_optional(tx.conn())
            .await?;
        let Some((account, revision, kind, hash, params_version)) = row else {
            return Ok(None);
        };
        let hash = <[u8; 32]>::try_from(hash).map_err(|_| overflow("content hash"))?;
        Ok::<_, DbError>(Some(HeldJob {
            account,
            revision,
            kind: parse_kind(&kind)?,
            content_hash: FileHash::from_bytes(hash),
            params_version: u32::try_from(params_version)
                .map_err(|_| overflow("params_version"))?,
        }))
    })
    .await
}

pub(crate) async fn fail(
    db: &Db,
    job: Uuid,
    plan: &FailPlan<'_>,
) -> Result<Failed, RunError<DbError>> {
    db.run_system("job_fail", async |tx| {
        job_fail_repo::fail_in(tx, job, plan).await
    })
    .await
}

pub(crate) async fn reap(
    db: &Db,
    now: Timestamp,
    max_attempts: i64,
) -> Result<Vec<Closed>, RunError<DbError>> {
    db.run_system("job_reap", async |tx| {
        let rows: Vec<ClosedRow> = sqlx::query_as(REAP)
            .bind(micros(now))
            .bind(max_attempts)
            .fetch_all(tx.conn())
            .await?;
        rows.into_iter().map(closed).collect()
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

/// System scope: `worker_status` is installation-level and has no account.
pub(crate) async fn record_status(
    db: &Db,
    worker: &str,
    seen_at: Timestamp,
    image_age_days: i32,
    decoder_ok: bool,
) -> Result<(), RunError<DbError>> {
    db.run_system("worker_status_record", async |tx| {
        sqlx::query(RECORD_STATUS)
            .bind(worker)
            .bind(micros(seen_at))
            .bind(image_age_days)
            .bind(decoder_ok)
            .execute(tx.conn())
            .await?;
        Ok::<_, DbError>(())
    })
    .await
}

/// The `JobReaper`: `worker_status` rows not seen since `before` are deleted.
pub(crate) async fn purge_worker_status(
    db: &Db,
    before: Timestamp,
) -> Result<u64, RunError<DbError>> {
    db.run_system("worker_status_purge", async |tx| {
        maintenance_repo::purge_worker_status(tx, before).await
    })
    .await
}

/// One batch of the `DerivedJanitor`, across accounts: up to `limit` derived rows whose content has no revision
/// left, their blobs queued `Due::Now` and the rows deleted in one transaction. Returns the rows deleted.
pub(crate) async fn remove_orphan_derived(
    db: &Db,
    limit: u32,
    gc: &dyn GcQueue,
) -> Result<u64, RunError<Fault>> {
    db.run_system("derived_janitor", async |tx| {
        let blobs = maintenance_repo::lock_orphans(tx, limit).await?;
        maintenance_repo::queue_and_delete(tx, gc, &blobs, GcReason::DerivedRemoved).await
    })
    .await
}

/// `MediaAdmin::stats` and the queue gauges: job counts per lane and status over every account.
pub(crate) async fn job_counts(db: &Db) -> Result<Vec<JobCountRow>, RunError<DbError>> {
    db.run_system("job_counts", async |tx| admin_repo::job_counts(tx).await)
        .await
}

/// The doctor check and the health item: the worker rows seen since `since` and the queued and dead jobs.
pub(crate) async fn worker_facts(
    db: &Db,
    since: Timestamp,
) -> Result<WorkerFacts, RunError<DbError>> {
    db.run_system("worker_facts", async |tx| {
        admin_repo::worker_facts(tx, since).await
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
