//! The statements on `share_links` for the owner operations. Each runs on the caller's transaction in the owner's
//! account scope and names the account, so row-level security and the statement agree. Only the token's hash and
//! the password's PHC string reach this module.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::TokenHash;
use sqlx::Row;
use uuid::Uuid;

const INSERT: &str = "INSERT INTO share_links \
    (id, account_id, node_id, token_hash, password_hash, expires_at, created_at) \
    VALUES ($1, $2, $3, $4, $5, \
    'epoch'::timestamptz + $6::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $7::bigint * interval '1 microsecond')";

// Revoked links are left out: `ShareInfo` cannot show a revocation, and a revoked link never works again.
const LIST: &str = "SELECT id, node_id, (extract(epoch FROM expires_at) * 1000000)::bigint, \
    password_hash IS NOT NULL, (extract(epoch FROM created_at) * 1000000)::bigint \
    FROM share_links \
    WHERE account_id = $1 AND revoked_at IS NULL AND ($2::uuid IS NULL OR node_id = $2) \
    ORDER BY created_at, id";

// Only the first revoke matches, so only it returns the node for the audit event.
const REVOKE: &str = "UPDATE share_links \
    SET revoked_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond' \
    WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL \
    RETURNING node_id";

const EXISTS: &str = "SELECT EXISTS (SELECT 1 FROM share_links WHERE id = $1 AND account_id = $2)";

/// A new link row; the plaintext token and password never reach this module.
pub(crate) struct NewLink<'a> {
    pub id: Uuid,
    pub account_id: Uuid,
    pub node_id: Uuid,
    pub token_hash: &'a TokenHash,
    pub password_hash: Option<&'a str>,
    pub expires_at: Timestamp,
    pub created_at: Timestamp,
}

/// A listed link, without its token hash or password hash.
pub(crate) struct ListedLink {
    pub id: Uuid,
    pub node_id: Uuid,
    pub expires_at_micros: i64,
    pub has_password: bool,
    pub created_at_micros: i64,
}

pub(crate) async fn insert(tx: &mut DbTx, link: &NewLink<'_>) -> Result<(), DbError> {
    sqlx::query(INSERT)
        .bind(link.id)
        .bind(link.account_id)
        .bind(link.node_id)
        .bind(link.token_hash.as_bytes().as_slice())
        .bind(link.password_hash)
        .bind(link.expires_at.as_unix_micros())
        .bind(link.created_at.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn list(
    tx: &mut DbTx,
    account_id: Uuid,
    node_id: Option<Uuid>,
) -> Result<Vec<ListedLink>, DbError> {
    let rows = sqlx::query(LIST)
        .bind(account_id)
        .bind(node_id)
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(ListedLink {
                id: row.try_get(0)?,
                node_id: row.try_get(1)?,
                expires_at_micros: row.try_get(2)?,
                has_password: row.try_get(3)?,
                created_at_micros: row.try_get(4)?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(DbError::from)
}

/// The node of the link when this call revoked it; `None` when the link is already revoked or not the account's.
pub(crate) async fn revoke(
    tx: &mut DbTx,
    account_id: Uuid,
    link_id: Uuid,
    at: Timestamp,
) -> Result<Option<Uuid>, DbError> {
    let node: Option<Uuid> = sqlx::query_scalar(REVOKE)
        .bind(link_id)
        .bind(account_id)
        .bind(at.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    Ok(node)
}

pub(crate) async fn exists(
    tx: &mut DbTx,
    account_id: Uuid,
    link_id: Uuid,
) -> Result<bool, DbError> {
    let found: bool = sqlx::query_scalar(EXISTS)
        .bind(link_id)
        .bind(account_id)
        .fetch_one(tx.conn())
        .await?;
    Ok(found)
}
