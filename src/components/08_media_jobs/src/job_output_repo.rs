//! The job statements of `accept_output`'s one short transaction, run on the caller's account-scoped transaction.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use uuid::Uuid;

use crate::job_repo::{at, micros};

/// The holder check of `job_repo::HELD` (closed `derived` allowed) again, now with the row locked: concurrent posts for one job
/// run their transactions one after the other. The column is true while the job is running.
const LOCK_FOR_OUTPUT: &str = concat!(
    "SELECT status = 'running' FROM jobs WHERE id = $1 AND account_id = $2 AND leased_by = $3 \
     AND ((status = 'running' AND available_at > ",
    at!("$4"),
    ") OR (status = 'succeeded' AND outcome = 'derived')) FOR UPDATE"
);

const CLOSE_DERIVED: &str = concat!(
    "UPDATE jobs SET status = 'succeeded', outcome = 'derived', finished_at = ",
    at!("$3"),
    " WHERE id = $1 AND account_id = $2 AND status = 'running'"
);

/// `None` when the worker no longer holds the job; otherwise whether it is still running.
pub(crate) async fn lock_for_output(
    tx: &mut DbTx,
    account: Uuid,
    job: Uuid,
    worker: &str,
    now: Timestamp,
) -> Result<Option<bool>, DbError> {
    Ok(sqlx::query_scalar(LOCK_FOR_OUTPUT)
        .bind(job)
        .bind(account)
        .bind(worker)
        .bind(micros(now))
        .fetch_optional(tx.conn())
        .await?)
}

pub(crate) async fn close_derived(
    tx: &mut DbTx,
    account: Uuid,
    job: Uuid,
    now: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(CLOSE_DERIVED)
        .bind(job)
        .bind(account)
        .bind(micros(now))
        .execute(tx.conn())
        .await?;
    Ok(())
}
