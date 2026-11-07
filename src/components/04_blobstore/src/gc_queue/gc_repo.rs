//! The statements of the GC queue. `schedule` is the only writer of `blob_gc` and of `blobs.state = 'gc_pending'`.

use sc_db::{Db, DbError, DbTx, RunError, Scope};
use uuid::Uuid;

/// One statement, so the state and the queue row change together. A queued blob keeps one row: the earliest
/// `due_at`, with the reason that came with it.
const SCHEDULE: &str = "WITH queued AS ( \
         UPDATE blobs SET state = 'gc_pending' WHERE id = $1 RETURNING id, account_id) \
     INSERT INTO blob_gc (blob_id, account_id, due_at, reason) \
     SELECT id, account_id, 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', $3 FROM queued \
     ON CONFLICT (blob_id) DO UPDATE SET \
         reason = CASE WHEN EXCLUDED.due_at < blob_gc.due_at THEN EXCLUDED.reason ELSE blob_gc.reason END, \
         due_at = LEAST(blob_gc.due_at, EXCLUDED.due_at)";

const PENDING: &str = "SELECT count(*) FROM blob_gc WHERE account_id = $1";

/// True when the blob row exists and is now queued.
pub(super) async fn schedule(
    tx: &mut DbTx,
    blob_id: Uuid,
    due_micros: i64,
    reason: &'static str,
) -> Result<bool, DbError> {
    let done = sqlx::query(SCHEDULE)
        .bind(blob_id)
        .bind(due_micros)
        .bind(reason)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

pub(super) async fn pending(db: &Db, account: Uuid) -> Result<i64, RunError<DbError>> {
    db.run(Scope::Account(account), async |tx| {
        let count: i64 = sqlx::query_scalar(PENDING)
            .bind(account)
            .fetch_one(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(count)
    })
    .await
}
