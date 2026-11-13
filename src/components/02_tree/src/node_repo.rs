//! Statements of the folder, rename, move and delete operations. Every statement names `account_id`, so a foreign
//! id behaves like a random one (row-level security is the second line).

use contracts::{NodeKind, NodeState, NodeView};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::FileHash;
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

/// The columns of one node that the mutations decide on.
pub(crate) struct NodeRow {
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub deleted: bool,
    pub current_revision_id: Option<Uuid>,
}

/// A folder about to be inserted; `name_key` comes from `sc_names::name_key`.
pub(crate) struct NewFolder<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub parent_id: Uuid,
    pub name: &'a str,
    pub name_key: &'a str,
    pub now: Timestamp,
}

/// A rename and/or move of one node; `None` keeps the stored value.
pub(crate) struct NodeChange<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub parent_id: Uuid,
    pub name: Option<&'a str>,
    pub name_key: Option<&'a str>,
    pub now: Timestamp,
}

const NODE_FOR_UPDATE: &str = "SELECT parent_id, kind, deleted_at IS NOT NULL, current_revision_id \
    FROM nodes WHERE id = $1 AND account_id = $2 FOR UPDATE";

const NODE_FOR_SHARE: &str = "SELECT parent_id, kind, deleted_at IS NOT NULL, current_revision_id \
    FROM nodes WHERE id = $1 AND account_id = $2 FOR SHARE";

const INSERT_FOLDER: &str = "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, \
    name_key_version, created_at, updated_at) VALUES ($1, $2, $3, 'folder', $4, $5, $6, \
    'epoch'::timestamptz + $7::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $7::bigint * interval '1 microsecond')";

const UPDATE_NODE: &str = "UPDATE nodes SET parent_id = $3, name = COALESCE($4, name), \
    name_key = COALESCE($5, name_key), \
    name_key_version = CASE WHEN $5::text IS NULL THEN name_key_version ELSE $6 END, \
    updated_at = 'epoch'::timestamptz + $7::bigint * interval '1 microsecond' \
    WHERE id = $1 AND account_id = $2";

const TOMBSTONE: &str = "UPDATE nodes SET \
    deleted_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
    updated_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
    cascade_pending = (kind = 'folder') WHERE id = $1 AND account_id = $2";

// Walks up from the move target. The tree is acyclic (every move runs this check under the account lock), so the
// walk ends at the root; it also stops at the moved node, whose presence in the chain is the cycle.
const REACHES_NODE: &str = "WITH RECURSIVE chain (id, parent_id) AS ( \
    SELECT id, parent_id FROM nodes WHERE id = $1 AND account_id = $3 \
    UNION ALL \
    SELECT n.id, n.parent_id FROM nodes n JOIN chain c ON n.id = c.parent_id \
    WHERE n.account_id = $3 AND c.id <> $2) \
    SELECT EXISTS (SELECT 1 FROM chain WHERE id = $2)";

const VIEW: &str = "SELECT n.parent_id, n.kind, n.name, n.deleted_at IS NOT NULL, n.current_revision_id, \
    r.size, r.content_hash, r.media_type, \
    (extract(epoch FROM r.client_mtime) * 1000000)::bigint, \
    (extract(epoch FROM n.created_at) * 1000000)::bigint, \
    (extract(epoch FROM n.updated_at) * 1000000)::bigint, n.seq \
    FROM nodes n LEFT JOIN revisions r ON r.id = n.current_revision_id AND r.account_id = n.account_id \
    WHERE n.id = $1 AND n.account_id = $2";

pub(crate) async fn lock_for_update(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<NodeRow>, DbError> {
    lock(tx, NODE_FOR_UPDATE, account, id).await
}

/// The parent-row lock of a create or a move: it waits for a delete of that folder in flight, and then sees its
/// tombstone, so nothing is added under a folder whose tombstone committed first.
pub(crate) async fn lock_for_share(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<NodeRow>, DbError> {
    lock(tx, NODE_FOR_SHARE, account, id).await
}

async fn lock(
    tx: &mut DbTx,
    statement: &'static str,
    account: Uuid,
    id: Uuid,
) -> Result<Option<NodeRow>, DbError> {
    let row = sqlx::query(statement)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| {
        Ok(NodeRow {
            parent_id: row.try_get(0)?,
            kind: kind(&row, 1)?,
            deleted: row.try_get(2)?,
            current_revision_id: row.try_get(3)?,
        })
    })
    .transpose()
}

pub(crate) async fn insert_folder(tx: &mut DbTx, folder: &NewFolder<'_>) -> Result<(), DbError> {
    sqlx::query(INSERT_FOLDER)
        .bind(folder.id)
        .bind(folder.account)
        .bind(folder.parent_id)
        .bind(folder.name)
        .bind(folder.name_key)
        .bind(sc_names::NAME_KEY_VERSION)
        .bind(folder.now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn update(tx: &mut DbTx, change: &NodeChange<'_>) -> Result<(), DbError> {
    sqlx::query(UPDATE_NODE)
        .bind(change.id)
        .bind(change.account)
        .bind(change.parent_id)
        .bind(change.name)
        .bind(change.name_key)
        .bind(sc_names::NAME_KEY_VERSION)
        .bind(change.now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn tombstone(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    now: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(TOMBSTONE)
        .bind(id)
        .bind(account)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// Whether `node` is `target` or one of its ancestors: moving `node` under `target` would close a cycle.
pub(crate) async fn reaches(
    tx: &mut DbTx,
    account: Uuid,
    target: Uuid,
    node: Uuid,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(REACHES_NODE)
        .bind(target)
        .bind(node)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

pub(crate) async fn view(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<NodeView>, DbError> {
    let row = sqlx::query(VIEW)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| node_view(id, &row)).transpose()
}

pub(crate) fn node_view(id: Uuid, row: &PgRow) -> Result<NodeView, DbError> {
    let deleted: bool = row.try_get(3)?;
    let content_hash: Option<Vec<u8>> = row.try_get(6)?;
    let client_mtime: Option<i64> = row.try_get(8)?;
    Ok(NodeView {
        id,
        parent_id: row.try_get(0)?,
        kind: kind(row, 1)?,
        name: row.try_get(2)?,
        state: if deleted {
            NodeState::Tombstone
        } else {
            NodeState::Live
        },
        current_revision_id: row.try_get(4)?,
        size: row.try_get(5)?,
        content_hash: content_hash.map(file_hash).transpose()?,
        media_type: row.try_get(7)?,
        client_mtime: client_mtime.map(Timestamp::from_unix_micros),
        created_at: Timestamp::from_unix_micros(row.try_get(9)?),
        updated_at: Timestamp::from_unix_micros(row.try_get(10)?),
        seq: row.try_get(11)?,
    })
}

pub(crate) fn kind(row: &PgRow, index: usize) -> Result<NodeKind, DbError> {
    match row.try_get::<&str, _>(index)? {
        "file" => Ok(NodeKind::File),
        "folder" => Ok(NodeKind::Folder),
        other => Err(DbError::Other(format!("unknown node kind {other:?}"))),
    }
}

pub(crate) fn file_hash(bytes: Vec<u8>) -> Result<FileHash, DbError> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|bytes: Vec<u8>| {
        DbError::Other(format!("content hash of {} bytes", bytes.len()))
    })?;
    Ok(FileHash::from_bytes(bytes))
}
