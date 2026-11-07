//! The statements of the admin views, the limit and the lifecycle hook. Account-scoped statements run on the
//! caller's transaction.

use std::collections::HashMap;

use contracts::QuotaView;
use sc_db::{Db, DbError, DbTx, RunError};
use uuid::Uuid;

const VIEW: &str = "SELECT limit_bytes, COALESCE(limit_bytes, $2::bigint), used_bytes, reserved_bytes \
     FROM quota WHERE account_id = $1";

const VIEWS: &str = "SELECT account_id, limit_bytes, COALESCE(limit_bytes, $2::bigint), used_bytes, reserved_bytes \
     FROM quota WHERE account_id = ANY($1)";

const SET_LIMIT: &str = "UPDATE quota SET limit_bytes = $2 WHERE account_id = $1";

const INSERT: &str = "INSERT INTO quota (account_id) VALUES ($1)";

const DELETE: &str = "DELETE FROM quota WHERE account_id = $1";

fn quota_view(
    limit_bytes: Option<i64>,
    effective_limit: i64,
    used: i64,
    reserved: i64,
) -> QuotaView {
    QuotaView {
        limit_bytes,
        effective_limit,
        used,
        reserved,
    }
}

pub async fn view(
    tx: &mut DbTx,
    account: Uuid,
    default_limit: i64,
) -> Result<Option<QuotaView>, DbError> {
    let row: Option<(Option<i64>, i64, i64, i64)> = sqlx::query_as(VIEW)
        .bind(account)
        .bind(default_limit)
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(|(limit, effective, used, reserved)| quota_view(limit, effective, used, reserved)))
}

/// One statement for all ids. A row-level policy shows an account scope only its own row, so this is the one
/// read of the crate that runs in system scope; the caller names the ids.
pub async fn views(
    db: &Db,
    accounts: &[Uuid],
    default_limit: i64,
) -> Result<HashMap<Uuid, QuotaView>, RunError<DbError>> {
    db.run_system("quota_views", async |tx| {
        let rows: Vec<(Uuid, Option<i64>, i64, i64, i64)> = sqlx::query_as(VIEWS)
            .bind(accounts)
            .bind(default_limit)
            .fetch_all(tx.conn())
            .await?;
        Ok::<_, DbError>(
            rows.into_iter()
                .map(|(account, limit, effective, used, reserved)| {
                    (account, quota_view(limit, effective, used, reserved))
                })
                .collect(),
        )
    })
    .await
}

/// Whether the account has a row.
pub async fn set_limit(tx: &mut DbTx, account: Uuid, limit: Option<i64>) -> Result<bool, DbError> {
    let done = sqlx::query(SET_LIMIT)
        .bind(account)
        .bind(limit)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn insert(tx: &mut DbTx, account: Uuid) -> Result<(), DbError> {
    sqlx::query(INSERT).bind(account).execute(tx.conn()).await?;
    Ok(())
}

pub async fn delete(tx: &mut DbTx, account: Uuid) -> Result<(), DbError> {
    sqlx::query(DELETE).bind(account).execute(tx.conn()).await?;
    Ok(())
}
