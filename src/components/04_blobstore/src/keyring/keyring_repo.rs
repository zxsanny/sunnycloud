//! The statements of the keyring. `blobs` is read in system scope because the key check covers the rows of every
//! account; each call is one short transaction, closed before any file is touched.

use contracts::KeyError;
use sc_db::{Db, DbError};
use uuid::Uuid;

use crate::error::{BlobIds, key_error_from_db};

const ANY_BLOB: &str = "SELECT EXISTS (SELECT 1 FROM blobs)";
const SAMPLE: &str =
    "SELECT id, wrapped_key FROM blobs WHERE state = 'committed' ORDER BY random() LIMIT $1";

/// True when `blobs` has a row in any state.
pub(crate) async fn any_blob(db: &Db) -> Result<bool, KeyError> {
    db.run_system("keyring_any_blob", async |tx| {
        let exists: bool = sqlx::query_scalar(ANY_BLOB)
            .fetch_one(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(exists)
    })
    .await
    .map_err(|error| key_error_from_db(error, BlobIds::default()))
}

/// Up to `limit` random committed blobs: id and wrapped data key.
pub(crate) async fn sample_committed(
    db: &Db,
    limit: i64,
) -> Result<Vec<(Uuid, Vec<u8>)>, KeyError> {
    db.run_system("keyring_key_sample", async |tx| {
        let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(SAMPLE)
            .bind(limit)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(rows)
    })
    .await
    .map_err(|error| key_error_from_db(error, BlobIds::default()))
}
