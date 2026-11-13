//! The statements on `derived` that run inside the caller's account-scoped transaction.

use media_shared::types::DerivedKind;
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::FileHash;
use uuid::Uuid;

use crate::derived_access_repo::derived_kind_name;
use crate::job_repo::{at, micros};

/// The derived kinds stored for one content at one params version: a probe on the `derived_pkey` prefix
/// (account_id, content_hash), at most one row per kind.
const KINDS_PRESENT: &str = "SELECT kind FROM derived WHERE account_id = $1 AND content_hash = $2 \
     AND params_version = $3::bigint";

/// Exactly one row per (account, content hash, kind, params version): a second insert of the key does nothing.
const INSERT: &str = concat!(
    "INSERT INTO derived (account_id, content_hash, kind, params_version, blob_id, media_type, size, width, \
     height, duration_ms, created_at) VALUES ($1, $2, $3, $4::bigint, $5, $6, $7, $8, $9, $10, ",
    at!("$11"),
    ") ON CONFLICT ON CONSTRAINT derived_pkey DO NOTHING"
);

/// One stored output. The media type is the sniffed one, never the worker's claim.
pub(crate) struct NewDerived<'a> {
    pub account: Uuid,
    pub content_hash: &'a FileHash,
    pub kind: DerivedKind,
    pub params_version: u32,
    pub blob_id: Uuid,
    pub media_type: &'static str,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i32>,
    pub created_at: Timestamp,
}

/// False when a row of the key already exists.
pub(crate) async fn insert(tx: &mut DbTx, row: &NewDerived<'_>) -> Result<bool, DbError> {
    let done = sqlx::query(INSERT)
        .bind(row.account)
        .bind(row.content_hash.as_bytes().as_slice())
        .bind(derived_kind_name(row.kind))
        .bind(i64::from(row.params_version))
        .bind(row.blob_id)
        .bind(row.media_type)
        .bind(row.size)
        .bind(row.width)
        .bind(row.height)
        .bind(row.duration_ms)
        .bind(micros(row.created_at))
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

pub(crate) async fn kinds_present(
    tx: &mut DbTx,
    account: Uuid,
    content_hash: &FileHash,
    params_version: u32,
) -> Result<Vec<DerivedKind>, DbError> {
    let names: Vec<String> = sqlx::query_scalar(KINDS_PRESENT)
        .bind(account)
        .bind(content_hash.as_bytes().as_slice())
        .bind(i64::from(params_version))
        .fetch_all(tx.conn())
        .await?;
    names.iter().map(|name| parse_kind(name)).collect()
}

fn parse_kind(name: &str) -> Result<DerivedKind, DbError> {
    match name {
        "thumb" => Ok(DerivedKind::Thumb),
        "preview" => Ok(DerivedKind::Preview),
        "transcode" => Ok(DerivedKind::Transcode),
        other => Err(DbError::Other(format!("unknown derived kind {other:?}"))),
    }
}
