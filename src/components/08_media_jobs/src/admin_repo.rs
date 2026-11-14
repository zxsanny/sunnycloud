//! The read statements of the operator and health surface (queue counts, worker presence), run on the
//! system-scope transactions `job_repo` opens. They read the tables only.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};

use crate::job_repo::{at, micros, overflow};

/// Every job is counted: one sequential pass over `jobs`, which `job_terminal_ttl` keeps to about two weeks of jobs.
const JOB_COUNTS: &str =
    "SELECT lane, status, count(*) FROM jobs GROUP BY lane, status ORDER BY lane, status";

const WORKER_FACTS: &str = concat!(
    "SELECT count(*), count(*) FILTER (WHERE NOT decoder_ok), coalesce(max(image_age_days), 0), \
     (SELECT count(*) FROM jobs WHERE status = 'queued'), (SELECT count(*) FROM jobs WHERE status = 'dead') \
     FROM worker_status WHERE seen_at >= ",
    at!("$1")
);

/// lane, status, count.
pub(crate) type JobCountRow = (String, String, i64);

/// What the worker presence rules read: the rows seen since the cut-off, and the jobs that wait on a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkerFacts {
    /// Worker rows seen at or after the cut-off.
    pub fresh: u64,
    /// Fresh rows whose decoder self-check failed.
    pub decoder_failing: u64,
    /// The oldest decoder image among the fresh rows, in days; 0 without a fresh row.
    pub image_age_days: i32,
    pub queued: u64,
    pub dead: u64,
}

pub(crate) async fn job_counts(tx: &mut DbTx) -> Result<Vec<JobCountRow>, DbError> {
    Ok(sqlx::query_as(JOB_COUNTS).fetch_all(tx.conn()).await?)
}

pub(crate) async fn worker_facts(tx: &mut DbTx, since: Timestamp) -> Result<WorkerFacts, DbError> {
    let (fresh, failing, age, queued, dead): (i64, i64, i32, i64, i64) =
        sqlx::query_as(WORKER_FACTS)
            .bind(micros(since))
            .fetch_one(tx.conn())
            .await?;
    let count = |value: i64| u64::try_from(value).map_err(|_| overflow("count"));
    Ok(WorkerFacts {
        fresh: count(fresh)?,
        decoder_failing: count(failing)?,
        image_age_days: age,
        queued: count(queued)?,
        dead: count(dead)?,
    })
}
