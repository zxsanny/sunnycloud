//! Statements on `vaults`. Every statement names `account_id`; reads and writes of one vault are primary-key
//! lookups.
//!
//! The header ETag is PostgreSQL's SHA-256 of the stored column (`encode(sha256(header), 'hex')`), so it is
//! always the digest of the bytes as stored, and a replace compares it inside its own `UPDATE`: the row lock
//! and the re-check of the condition on the newest row version decide a race, never a read made earlier.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

// Without a conflict target, either unique key (`account_id`: one vault per account, `root_node_id`) makes the
// insert a no-op that returns no row.
const INSERT: &str = "INSERT INTO vaults (id, account_id, root_node_id, header, created_at, updated_at) \
    VALUES ($1, $2, $3, $4, \
    'epoch'::timestamptz + $5::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $5::bigint * interval '1 microsecond') \
    ON CONFLICT DO NOTHING \
    RETURNING encode(sha256(header), 'hex')";

const GET: &str = "SELECT header, root_node_id, encode(sha256(header), 'hex') \
    FROM vaults WHERE id = $1 AND account_id = $2";

const REPLACE: &str = "UPDATE vaults \
    SET header = $3, updated_at = 'epoch'::timestamptz + $5::bigint * interval '1 microsecond' \
    WHERE id = $1 AND account_id = $2 AND encode(sha256(header), 'hex') = $4 \
    RETURNING encode(sha256(header), 'hex')";

const EXISTS: &str = "SELECT EXISTS (SELECT 1 FROM vaults WHERE id = $1 AND account_id = $2)";

const DELETE_BATCH: &str = "\
DELETE FROM vaults WHERE id IN (
    SELECT id FROM vaults WHERE account_id = $1 ORDER BY id LIMIT $2
)";

const COUNT: &str = "SELECT count(*) FROM vaults WHERE account_id = $1";

pub(crate) struct NewVault<'a> {
    pub id: Uuid,
    pub account_id: Uuid,
    pub root_node_id: Uuid,
    pub header: &'a [u8],
    pub created_at: Timestamp,
}

pub(crate) struct StoredHeader {
    pub header: Vec<u8>,
    pub root_node_id: Uuid,
    pub etag: String,
}

/// The ETag of the stored header, or `None` when the account already has a vault or the root is a vault root.
pub(crate) async fn insert(tx: &mut DbTx, vault: &NewVault<'_>) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar(INSERT)
        .bind(vault.id)
        .bind(vault.account_id)
        .bind(vault.root_node_id)
        .bind(vault.header)
        .bind(vault.created_at.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?)
}

pub(crate) async fn get(
    tx: &mut DbTx,
    account: Uuid,
    vault_id: Uuid,
) -> Result<Option<StoredHeader>, DbError> {
    let row = sqlx::query(GET)
        .bind(vault_id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| {
        Ok(StoredHeader {
            header: row.try_get(0)?,
            root_node_id: row.try_get(1)?,
            etag: row.try_get(2)?,
        })
    })
    .transpose()
}

/// The ETag of the new header, or `None` when the vault is not the account's or its stored header's ETag is not
/// `if_match`.
pub(crate) async fn replace(
    tx: &mut DbTx,
    account: Uuid,
    vault_id: Uuid,
    header: &[u8],
    if_match: &str,
    now: Timestamp,
) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar(REPLACE)
        .bind(vault_id)
        .bind(account)
        .bind(header)
        .bind(if_match)
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?)
}

pub(crate) async fn exists(tx: &mut DbTx, account: Uuid, vault_id: Uuid) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(EXISTS)
        .bind(vault_id)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

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
