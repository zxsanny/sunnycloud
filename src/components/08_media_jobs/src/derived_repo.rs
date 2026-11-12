//! The statements on `derived` that run inside the caller's account-scoped transaction.

use media_shared::types::DerivedKind;
use sc_db::{DbError, DbTx};
use sc_ids::FileHash;
use uuid::Uuid;

/// The derived kinds stored for one content at one params version: a probe on the `derived_pkey` prefix
/// (account_id, content_hash), at most one row per kind.
const KINDS_PRESENT: &str = "SELECT kind FROM derived WHERE account_id = $1 AND content_hash = $2 \
     AND params_version = $3::bigint";

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
