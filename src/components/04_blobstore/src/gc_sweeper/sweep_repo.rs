//! The statements of the GC sweeper. The sweeper works across accounts, so every unit of work runs in system scope
//! (ADR-014). Each is one short transaction; none spans a file operation.

use std::sync::Arc;

use contracts::{BlobOwnerCleanup, BlobRegistry as _, Fault};
use sc_db::{Db, DbError, DbTx, RunError};
use uuid::Uuid;

use crate::error::{BlobIds, db_fault};
use crate::registry::BlobRegistry;

/// The blob row is locked together with its queue row, in the order the reinstate path takes them (blob first).
/// `SKIP LOCKED` leaves a row that a reinstate or another sweeper holds, so the claim never waits.
const CLAIM: &str = "WITH due AS ( \
         SELECT g.blob_id FROM blobs b JOIN blob_gc g ON g.blob_id = b.id \
         WHERE g.due_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
             AND b.state = 'gc_pending' \
         ORDER BY g.due_at, g.blob_id LIMIT $3 \
         FOR UPDATE OF b, g SKIP LOCKED) \
     UPDATE blob_gc SET due_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
     FROM due WHERE blob_gc.blob_id = due.blob_id \
     RETURNING blob_gc.blob_id";

/// Locks the blob rows (blocking: a reinstate in flight finishes first) and tells whether the queue row still
/// carries this sweeper's lease. A re-queued blob has a different `due_at`. The owner account is what the
/// registry's row removal is filtered by.
const LOCK: &str = "SELECT b.id, b.account_id, b.state, \
         (SELECT g.due_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
          FROM blob_gc g WHERE g.blob_id = b.id) \
     FROM blobs b WHERE b.id = ANY($1) ORDER BY b.id FOR UPDATE OF b";

const COUNT_DUE: &str = "SELECT count(*) FROM blobs b JOIN blob_gc g ON g.blob_id = b.id \
     WHERE g.due_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
         AND b.state = 'gc_pending'";

/// The failure of the work done for one blob inside its savepoint: a database error or a refusal of a
/// cleanup or of the row removal.
pub(super) struct RowError(pub Fault);

impl From<DbError> for RowError {
    fn from(error: DbError) -> Self {
        RowError(db_fault(error, BlobIds::default()))
    }
}

pub(super) enum Verdict {
    /// The cleanups ran and the rows are gone.
    Removed,
    /// The row exists but is no longer this sweeper's to remove (reinstated, or queued again).
    Kept,
    /// The row was already removed by someone else.
    Gone,
    /// The cleanups or the removal failed; the savepoint was rolled back and the row stays.
    Failed(Fault),
}

pub(super) async fn claim(
    db: &Db,
    now_micros: i64,
    lease_until_micros: i64,
    limit: i64,
) -> Result<Vec<Uuid>, RunError<DbError>> {
    db.run_system("blob_gc_claim", async |tx| {
        let ids: Vec<Uuid> = sqlx::query_scalar(CLAIM)
            .bind(now_micros)
            .bind(lease_until_micros)
            .bind(limit)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(ids)
    })
    .await
}

pub(super) async fn count_due(db: &Db, now_micros: i64) -> Result<i64, RunError<DbError>> {
    db.run_system("blob_gc_count", async |tx| {
        let count: i64 = sqlx::query_scalar(COUNT_DUE)
            .bind(now_micros)
            .fetch_one(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(count)
    })
    .await
}

/// The cleanups of every registered owner, then the blob's own rows.
async fn remove_one(
    tx: &mut DbTx,
    cleanups: &[Arc<dyn BlobOwnerCleanup>],
    registry: &BlobRegistry,
    account: Uuid,
    blob_id: Uuid,
) -> Result<(), RowError> {
    for cleanup in cleanups {
        cleanup
            .remove_owner_rows(tx, blob_id)
            .await
            .map_err(|error| RowError(Box::new(error)))?;
    }
    registry
        .remove_row_in_tx(tx, account, blob_id)
        .await
        .map_err(|error| RowError(Box::new(error)))
}

/// One transaction for a claimed batch. The cleanups and the row removal of one blob run in a savepoint, so one
/// blob's failure leaves the others of the batch committed.
pub(super) async fn settle(
    db: &Db,
    ids: &[Uuid],
    lease_until_micros: i64,
    cleanups: &[Arc<dyn BlobOwnerCleanup>],
    registry: &BlobRegistry,
) -> Result<Vec<(Uuid, Verdict)>, RunError<DbError>> {
    db.run_system("blob_gc_settle", async |tx| {
        let locked: Vec<(Uuid, Uuid, String, Option<bool>)> = sqlx::query_as(LOCK)
            .bind(ids)
            .bind(lease_until_micros)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        let mut verdicts = Vec::with_capacity(ids.len());
        for &id in ids {
            let verdict = match locked.iter().find(|(locked_id, ..)| *locked_id == id) {
                None => Verdict::Gone,
                Some((_, _, state, leased)) if state != "gc_pending" || *leased != Some(true) => {
                    Verdict::Kept
                }
                Some(&(_, account, ..)) => match tx
                    .savepoint(async |tx| remove_one(tx, cleanups, registry, account, id).await)
                    .await
                {
                    Ok(()) => Verdict::Removed,
                    Err(RowError(fault)) => Verdict::Failed(fault),
                },
            };
            verdicts.push((id, verdict));
        }
        Ok::<_, DbError>(verdicts)
    })
    .await
}
