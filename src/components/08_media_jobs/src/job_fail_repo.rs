//! The statements of a failure report, run on the system-scope transaction `job_repo::fail` opens: the holder row
//! is locked and read, then closed `dead` or put back to `queued` with its backoff.

use std::time::Duration;

use media_shared::types::Disposition;
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use uuid::Uuid;

use crate::job_repo::{Closed, ClosedRow, at, closed, closed_columns, micros, overflow};
use crate::retry;

const LOCK_HOLDER: &str = concat!(
    "SELECT attempts FROM jobs WHERE id = $1 AND status = 'running' AND leased_by = $2 AND available_at > ",
    at!("$3"),
    " FOR UPDATE"
);

const MARK_DEAD: &str = concat!(
    "UPDATE jobs SET status = 'dead', finished_at = ",
    at!("$2"),
    ", last_error = $3 WHERE id = $1 RETURNING ",
    closed_columns!()
);

const REQUEUE_FAILED: &str = concat!(
    "UPDATE jobs SET status = 'queued', available_at = ",
    at!("$2"),
    ", leased_by = NULL, last_error = $3 WHERE id = $1"
);

pub(crate) enum Failed {
    LeaseLost,
    Dead(Closed),
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

pub(crate) async fn fail_in(
    tx: &mut DbTx,
    job: Uuid,
    plan: &FailPlan<'_>,
) -> Result<Failed, DbError> {
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
    if retry::is_dead(plan.disposition, i64::from(attempts), plan.max_attempts) {
        let row: ClosedRow = sqlx::query_as(MARK_DEAD)
            .bind(job)
            .bind(micros(plan.now))
            .bind(plan.error)
            .fetch_one(tx.conn())
            .await?;
        return Ok(Failed::Dead(closed(row)?));
    }
    let retry_at = retry::add(plan.now, retry::retry_delay(plan.backoff, attempts));
    sqlx::query(REQUEUE_FAILED)
        .bind(job)
        .bind(micros(retry_at))
        .bind(plan.error)
        .execute(tx.conn())
        .await?;
    Ok(Failed::Requeued)
}
