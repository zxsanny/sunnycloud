//! The two statements of the orphan sweeper. It works across accounts, so each runs in system scope (ADR-014) as
//! one short transaction, never open while a directory is listed or a file is removed.

use std::collections::HashSet;

use sc_db::{Db, DbError, RunError};
use uuid::Uuid;

/// Every row whose id lies in `[lo, hi]`, whatever its state or account: a file with any row is not an orphan.
const IN_RANGE: &str = "SELECT id FROM blobs WHERE id >= $1 AND id <= $2";

const EXISTS: &str = "SELECT EXISTS (SELECT 1 FROM blobs WHERE id = $1)";

pub(super) async fn ids_in_range(
    db: &Db,
    lo: Uuid,
    hi: Uuid,
) -> Result<HashSet<Uuid>, RunError<DbError>> {
    db.run_system("blob_orphan_range", async |tx| {
        let ids: Vec<Uuid> = sqlx::query_scalar(IN_RANGE)
            .bind(lo)
            .bind(hi)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(ids.into_iter().collect())
    })
    .await
}

pub(super) async fn row_exists(db: &Db, id: Uuid) -> Result<bool, RunError<DbError>> {
    db.run_system("blob_orphan_recheck", async |tx| {
        let exists: bool = sqlx::query_scalar(EXISTS)
            .bind(id)
            .fetch_one(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(exists)
    })
    .await
}
