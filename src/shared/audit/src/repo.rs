//! The only statements against `audit_log`: the writer's insert, the export select and the purge call
//! (`scripts/ci/check-audit-sql.sh` fails on any other). Timestamps are bound as microseconds since the epoch;
//! SQL never reads the clock.

use sc_clock::Timestamp;
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use uuid::Uuid;

use crate::event::{AuditEvent, AuditEventKind};

const INSERT: &str = "INSERT INTO audit_log \
    (at, actor_account_id, actor_session_id, event, subject_type, subject_id, ip, detail) \
    VALUES ('epoch'::timestamptz + $1::bigint * interval '1 microsecond', $2, $3, $4, $5, $6, \
    $7::text::inet, $8::text::jsonb)";

const EXPORT: &str = "SELECT id, (EXTRACT(EPOCH FROM at) * 1000000)::bigint, event, actor_account_id, \
    actor_session_id, subject_type, subject_id, host(ip), detail::text \
    FROM audit_log \
    WHERE at >= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
    AND ($2::text IS NULL OR event = $2) \
    AND id > $3 \
    ORDER BY id \
    LIMIT $4";

const PURGE: &str =
    "SELECT audit_log_purge('epoch'::timestamptz + $1::bigint * interval '1 microsecond')";

/// One exported row as the database returns it, before the shape check of the reader.
pub(crate) type RawRow = (
    i64,
    i64,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
);

/// `audit_log` is installation data without row-level security, so the unit of work carries no tenant: the
/// nil account matches no tenant row, which is the least scope `Db::run` offers.
pub(crate) async fn unscoped<T>(
    db: &Db,
    f: impl AsyncFnOnce(&mut DbTx) -> Result<T, DbError>,
) -> Result<T, DbError> {
    match db.run(Scope::Account(Uuid::nil()), f).await {
        Ok(value) => Ok(value),
        Err(RunError::Closure(error) | RunError::Db(error)) => Err(error),
    }
}

pub(crate) async fn insert(
    tx: &mut DbTx,
    at: Timestamp,
    event: &AuditEvent,
    detail: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query(INSERT)
        .bind(at.as_unix_micros())
        .bind(event.actor_account_id)
        .bind(event.actor_session_id)
        .bind(event.event.as_str())
        .bind(event.subject.map(|subject| subject.subject_type))
        .bind(event.subject.map(|subject| subject.id))
        .bind(event.ip.map(|ip| ip.to_string()))
        .bind(detail)
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn purge(tx: &mut DbTx, cutoff: Timestamp) -> Result<u64, DbError> {
    let removed: i64 = sqlx::query_scalar(PURGE)
        .bind(cutoff.as_unix_micros())
        .fetch_one(tx.conn())
        .await?;
    Ok(u64::try_from(removed).unwrap_or(0))
}

/// Up to `limit` rows with `id > after`, oldest id first.
pub(crate) async fn page(
    tx: &mut DbTx,
    since: Timestamp,
    event: Option<AuditEventKind>,
    after: i64,
    limit: i64,
) -> Result<Vec<RawRow>, DbError> {
    Ok(sqlx::query_as(EXPORT)
        .bind(since.as_unix_micros())
        .bind(event.map(AuditEventKind::as_str))
        .bind(after)
        .bind(limit)
        .fetch_all(tx.conn())
        .await?)
}
