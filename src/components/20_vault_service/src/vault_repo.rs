//! Statements on `vaults`. Every statement names `account_id`.

use sc_db::DbTx;
use uuid::Uuid;

const DELETE_BATCH: &str = "\
DELETE FROM vaults WHERE id IN (
    SELECT id FROM vaults WHERE account_id = $1 ORDER BY id LIMIT $2
)";

const COUNT: &str = "SELECT count(*) FROM vaults WHERE account_id = $1";

/// Deletes up to `limit` of the account's rows.
pub(crate) async fn delete_batch(
    tx: &mut DbTx,
    account: Uuid,
    limit: u32,
) -> Result<(), sqlx::Error> {
    sqlx::query(DELETE_BATCH)
        .bind(account)
        .bind(i64::from(limit))
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn count(tx: &mut DbTx, account: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(COUNT)
        .bind(account)
        .fetch_one(tx.conn())
        .await
}
