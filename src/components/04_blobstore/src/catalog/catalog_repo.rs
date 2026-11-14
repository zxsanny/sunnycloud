//! The reads of `blobs` on the request path: primary-key lookups filtered by account.

use sc_db::{Db, DbError, RunError, Scope};
use uuid::Uuid;

const GET_REF: &str = "SELECT size, wrapped_key, master_key_id FROM blobs \
     WHERE id = $1 AND account_id = $2 AND size IS NOT NULL";

const GET_UPLOADING_PLAN: &str = "SELECT wrapped_key, master_key_id FROM blobs \
     WHERE id = $1 AND account_id = $2 AND state = 'uploading'";

pub(super) type Row = (i64, Vec<u8>, i16);

pub(super) type KeyRow = (Vec<u8>, i16);

pub(super) async fn get_ref(
    db: &Db,
    account: Uuid,
    blob_id: Uuid,
) -> Result<Option<Row>, RunError<DbError>> {
    db.run(Scope::Account(account), async |tx| {
        let row: Option<Row> = sqlx::query_as(GET_REF)
            .bind(blob_id)
            .bind(account)
            .fetch_optional(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(row)
    })
    .await
}

pub(super) async fn get_uploading_plan(
    db: &Db,
    account: Uuid,
    blob_id: Uuid,
) -> Result<Option<KeyRow>, RunError<DbError>> {
    db.run(Scope::Account(account), async |tx| {
        let row: Option<KeyRow> = sqlx::query_as(GET_UPLOADING_PLAN)
            .bind(blob_id)
            .bind(account)
            .fetch_optional(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(row)
    })
    .await
}
