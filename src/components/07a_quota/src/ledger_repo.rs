//! The statements of the ledger. Each runs on the caller's transaction and is scoped by `account_id`.

use sc_db::{DbError, DbTx};
use uuid::Uuid;

/// The comparison is written as `bytes <= limit - used - reserved` so no input can overflow `bigint`.
const RESERVE: &str = "UPDATE quota SET reserved_bytes = reserved_bytes + $2::bigint \
     WHERE account_id = $1 \
     AND $2::bigint <= COALESCE(limit_bytes, $3::bigint) - used_bytes - reserved_bytes \
     RETURNING reserved_bytes";

const LIMITS: &str = "SELECT COALESCE(limit_bytes, $2::bigint), used_bytes, reserved_bytes \
     FROM quota WHERE account_id = $1";

const LOCK_RESERVED: &str = "SELECT reserved_bytes FROM quota WHERE account_id = $1 FOR UPDATE";

const RELEASE: &str = "UPDATE quota SET reserved_bytes = GREATEST(reserved_bytes - $2::bigint, 0) \
     WHERE account_id = $1";

/// The sum is checked in `numeric`, so a delta of any size is refused instead of overflowing.
const ADJUST: &str = "UPDATE quota SET used_bytes = used_bytes + $2::bigint \
     WHERE account_id = $1 \
     AND used_bytes::numeric + $2::bigint::numeric BETWEEN 0 AND 9223372036854775807";

const EXISTS: &str = "SELECT EXISTS (SELECT 1 FROM quota WHERE account_id = $1)";

/// Limit (with the default applied), used and reserved of a refused reservation.
pub struct Counters {
    pub limit: i64,
    pub used: i64,
    pub reserved: i64,
}

/// The reserved total after the update, or `None` when the row is absent or the reservation does not fit.
pub async fn reserve(
    tx: &mut DbTx,
    account: Uuid,
    bytes: i64,
    default_limit: i64,
) -> Result<Option<i64>, DbError> {
    Ok(sqlx::query_scalar(RESERVE)
        .bind(account)
        .bind(bytes)
        .bind(default_limit)
        .fetch_optional(tx.conn())
        .await?)
}

pub async fn counters(
    tx: &mut DbTx,
    account: Uuid,
    default_limit: i64,
) -> Result<Option<Counters>, DbError> {
    let row: Option<(i64, i64, i64)> = sqlx::query_as(LIMITS)
        .bind(account)
        .bind(default_limit)
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(|(limit, used, reserved)| Counters {
        limit,
        used,
        reserved,
    }))
}

/// Takes the row lock and returns the reserved total, or `None` when the row is absent.
pub async fn lock_reserved(tx: &mut DbTx, account: Uuid) -> Result<Option<i64>, DbError> {
    Ok(sqlx::query_scalar(LOCK_RESERVED)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?)
}

pub async fn release(tx: &mut DbTx, account: Uuid, bytes: i64) -> Result<(), DbError> {
    sqlx::query(RELEASE)
        .bind(account)
        .bind(bytes)
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// Whether the delta was applied.
pub async fn adjust(tx: &mut DbTx, account: Uuid, delta: i64) -> Result<bool, DbError> {
    let done = sqlx::query(ADJUST)
        .bind(account)
        .bind(delta)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn exists(tx: &mut DbTx, account: Uuid) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(EXISTS)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}
