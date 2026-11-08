//! The statements of the reader's findings. The reader gets no account (the `BlobReader` port has none), so the
//! row is reached in system scope; one short transaction, closed before any file is touched.

use sc_db::{Db, DbError, RunError};
use uuid::Uuid;

/// Only the first finding of a committed blob changes the row, so the caller writes its audit event only when
/// this returns the account.
const MARK: &str = "UPDATE blobs \
     SET unavailable_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
     WHERE id = $1 AND state = 'committed' AND unavailable_at IS NULL \
     RETURNING account_id";

const COUNT_UNAVAILABLE: &str = "SELECT count(*) FROM blobs WHERE unavailable_at IS NOT NULL";

/// Rows with `unavailable_at` set, in any state and of any account.
pub(crate) async fn count_unavailable(db: &Db) -> Result<i64, RunError<DbError>> {
    db.run_system("blob_health_unavailable", async |tx| {
        let count: i64 = sqlx::query_scalar(COUNT_UNAVAILABLE)
            .fetch_one(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(count)
    })
    .await
}

pub(super) async fn mark_unavailable(
    db: &Db,
    blob_id: Uuid,
    at_micros: i64,
) -> Result<Option<Uuid>, RunError<DbError>> {
    db.run_system("blob_reader_finding", async |tx| {
        let account: Option<Uuid> = sqlx::query_scalar(MARK)
            .bind(blob_id)
            .bind(at_micros)
            .fetch_optional(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(account)
    })
    .await
}
