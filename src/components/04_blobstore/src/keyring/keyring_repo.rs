//! The statements of the keyring. `blobs` is read and rewrapped in system scope because the key check and a
//! rotation cover the rows of every account; each call is one short transaction, closed before any file is
//! touched.

use contracts::KeyError;
use sc_db::{Db, DbError};
use uuid::Uuid;

use crate::error::{BlobIds, key_error_from_db};

const ANY_BLOB: &str = "SELECT EXISTS (SELECT 1 FROM blobs)";
const SAMPLE: &str =
    "SELECT id, wrapped_key FROM blobs WHERE state = 'committed' ORDER BY random() LIMIT $1";

/// Rows still wrapped under key `$1`, whatever their state: every one of them needs its key until it is rewrapped.
const UNDER_KEY: &str = "SELECT id, wrapped_key FROM blobs WHERE master_key_id = $1 LIMIT $2";

/// Moves the listed rows to key `$4`. A row is changed only while it still has the wrap that was read (`$3`
/// holds the old wraps, `$2` the new ones) under key `$5`, so a row someone else rewrapped or removed is left
/// alone and a second run changes nothing.
const REPLACE_WRAPS: &str = "UPDATE blobs AS b SET wrapped_key = t.new_wrap, master_key_id = $4 \
     FROM unnest($1::uuid[], $2::bytea[], $3::bytea[]) AS t(id, new_wrap, old_wrap) \
     WHERE b.id = t.id AND b.master_key_id = $5 AND b.wrapped_key = t.old_wrap";

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

/// Up to `limit` rows wrapped under `key_id`: id and wrapped data key.
pub(crate) async fn under_key(
    db: &Db,
    key_id: i16,
    limit: i64,
) -> Result<Vec<(Uuid, Vec<u8>)>, KeyError> {
    db.run_system("keyring_rewrap_read", async |tx| {
        let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(UNDER_KEY)
            .bind(key_id)
            .bind(limit)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(rows)
    })
    .await
    .map_err(|error| key_error_from_db(error, BlobIds::default()))
}

/// The rewrapped rows: `(blob id, wrap under the old key, wrap under the new key)`.
pub(crate) struct Rewrapped {
    pub ids: Vec<Uuid>,
    pub old_wraps: Vec<Vec<u8>>,
    pub new_wraps: Vec<Vec<u8>>,
}

/// One short transaction for the whole batch; returns the rows this call changed.
pub(crate) async fn replace_wraps(
    db: &Db,
    rows: &Rewrapped,
    from_id: i16,
    to_id: i16,
) -> Result<u64, KeyError> {
    db.run_system("keyring_rewrap_write", async |tx| {
        let done = sqlx::query(REPLACE_WRAPS)
            .bind(&rows.ids)
            .bind(&rows.new_wraps)
            .bind(&rows.old_wraps)
            .bind(to_id)
            .bind(from_id)
            .execute(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(done.rows_affected())
    })
    .await
    .map_err(|error| key_error_from_db(error, BlobIds::default()))
}
