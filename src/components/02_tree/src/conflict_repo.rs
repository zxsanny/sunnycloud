//! Statements of the conflict outcomes of `publish`: the name lookup behind conflict-copy names, the superseded
//! revision moved to its copy node, the tombstoned ancestors of a file and the revival of a node. Every statement
//! names `account_id`, so a foreign id behaves like a random one (row-level security is the second line).

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

/// A tombstoned folder above a committed file.
pub(crate) struct Tombstoned {
    pub id: Uuid,
    pub parent_id: Uuid,
    pub name: String,
}

const NAME_TAKEN: &str = "SELECT EXISTS (SELECT 1 FROM nodes \
    WHERE account_id = $1 AND parent_id = $2 AND name_key = $3 AND deleted_at IS NULL)";

const REPARENT_REVISION: &str =
    "UPDATE revisions SET node_id = $3 WHERE id = $1 AND account_id = $2";

// Walks the whole chain up to the root, not only to the first live folder: before the cascade has marked a
// subtree, a live folder can sit below a tombstoned one. The tree is acyclic, so the walk ends at the root.
const TOMBSTONED_ANCESTORS: &str = "WITH RECURSIVE chain (id, parent_id, name, deleted, depth) AS ( \
    SELECT id, parent_id, name, deleted_at IS NOT NULL, 1 FROM nodes WHERE id = $1 AND account_id = $2 \
    UNION ALL \
    SELECT n.id, n.parent_id, n.name, n.deleted_at IS NOT NULL, c.depth + 1 \
    FROM nodes n JOIN chain c ON n.id = c.parent_id WHERE n.account_id = $2) \
    SELECT id, parent_id, name FROM chain WHERE deleted AND parent_id IS NOT NULL ORDER BY depth DESC";

// Clearing `cascade_pending` stops the cascade under a revived folder: children it has not marked stay live.
const REVIVE: &str = "UPDATE nodes SET deleted_at = NULL, cascade_pending = false, \
    name = COALESCE($3, name), name_key = COALESCE($4, name_key), \
    name_key_version = CASE WHEN $4::text IS NULL THEN name_key_version ELSE $5 END, \
    updated_at = 'epoch'::timestamptz + $6::bigint * interval '1 microsecond' \
    WHERE id = $1 AND account_id = $2";

/// Whether a live node of the folder holds `name_key`.
pub(crate) async fn name_taken(
    tx: &mut DbTx,
    account: Uuid,
    parent: Uuid,
    name_key: &str,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(NAME_TAKEN)
        .bind(account)
        .bind(parent)
        .bind(name_key)
        .fetch_one(tx.conn())
        .await?)
}

/// Moves a revision row, blob and all, to another node; no bytes are copied.
pub(crate) async fn reparent_revision(
    tx: &mut DbTx,
    account: Uuid,
    revision: Uuid,
    node: Uuid,
) -> Result<(), DbError> {
    sqlx::query(REPARENT_REVISION)
        .bind(revision)
        .bind(account)
        .bind(node)
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// The tombstoned folders from `folder` up to the root, farthest first.
pub(crate) async fn tombstoned_ancestors(
    tx: &mut DbTx,
    account: Uuid,
    folder: Uuid,
) -> Result<Vec<Tombstoned>, DbError> {
    let rows = sqlx::query(TOMBSTONED_ANCESTORS)
        .bind(folder)
        .bind(account)
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(Tombstoned {
                id: row.try_get(0)?,
                parent_id: row.try_get(1)?,
                name: row.try_get(2)?,
            })
        })
        .collect()
}

/// Clears the tombstone and `cascade_pending` of a node, under a new name when one is given as `(name, name_key)`.
/// A taken name fails on the unique index; the caller runs this in a savepoint.
pub(crate) async fn revive(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    rename: Option<(&str, &str)>,
    now: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(REVIVE)
        .bind(id)
        .bind(account)
        .bind(rename.map(|(name, _)| name))
        .bind(rename.map(|(_, key)| key))
        .bind(sc_names::NAME_KEY_VERSION)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}
