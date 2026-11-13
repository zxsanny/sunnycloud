//! Statements of the reads (`PgTreeReader`): the root, a live child by `name_key`, a node with its current
//! revision and one listing page; and the child by `name_key` that ends a path (`TreeService::resolve_path`).
//! Every statement names `account_id`, so a foreign id behaves like a random one (row-level security is the second
//! line). None of them reads `blocklist`.

use contracts::{NodeKind, NodeView, RevisionView};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

use crate::node_repo::{file_hash, kind, node_view};

/// A node with its current revision (`None` for a folder), for `resolve_live_file`.
pub(crate) struct LiveFileRow {
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub deleted: bool,
    pub revision: Option<(RevisionView, Uuid)>,
}

/// One page of live children in `name_key` order, each with its key (the cursor of the next page).
pub(crate) struct ListedNode {
    pub view: NodeView,
    pub name_key: String,
}

const ROOT: &str = "SELECT id FROM nodes WHERE account_id = $1 AND parent_id IS NULL";

const LIVE_CHILD: &str = "SELECT id, kind FROM nodes \
    WHERE account_id = $1 AND parent_id = $2 AND name_key = $3 AND deleted_at IS NULL";

// The live child first; otherwise the most recently tombstoned one, since several tombstones can share a name.
const CHILD_BY_KEY: &str = "SELECT id, deleted_at IS NOT NULL FROM nodes \
    WHERE account_id = $1 AND parent_id = $2 AND name_key = $3 \
    ORDER BY deleted_at DESC NULLS FIRST, id DESC LIMIT 1";

const LIVE_FILE: &str = "SELECT n.parent_id, n.kind, n.deleted_at IS NOT NULL, r.id, r.size, r.content_hash, \
    r.media_type, (extract(epoch FROM r.client_mtime) * 1000000)::bigint, r.client_meta::text, \
    (extract(epoch FROM r.created_at) * 1000000)::bigint, r.blob_id \
    FROM nodes n LEFT JOIN revisions r ON r.id = n.current_revision_id AND r.account_id = n.account_id \
    WHERE n.id = $1 AND n.account_id = $2";

/// The listing page statement (data model 5.1, AC-6): a range scan of `nodes_parent_name_key` for one folder after
/// the cursor (`''` sorts before every key, so no cursor reads from the start), each row joined to its current
/// revision by primary key. `$4` is the page size plus one, the extra row telling whether a next page exists.
/// Public for the plan test only.
#[doc(hidden)]
pub const LISTING_PAGE_SQL: &str = "SELECT n.parent_id, n.kind, n.name, n.deleted_at IS NOT NULL, \
    n.current_revision_id, r.size, r.content_hash, r.media_type, \
    (extract(epoch FROM r.client_mtime) * 1000000)::bigint, \
    (extract(epoch FROM n.created_at) * 1000000)::bigint, \
    (extract(epoch FROM n.updated_at) * 1000000)::bigint, n.seq, n.id, n.name_key \
    FROM nodes n LEFT JOIN revisions r ON r.id = n.current_revision_id AND r.account_id = n.account_id \
    WHERE n.parent_id = $1 AND n.account_id = $2 AND n.deleted_at IS NULL \
    AND n.name_key > COALESCE($3, '') \
    ORDER BY n.name_key LIMIT $4";

pub(crate) async fn root(tx: &mut DbTx, account: Uuid) -> Result<Option<Uuid>, DbError> {
    Ok(sqlx::query_scalar(ROOT)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?)
}

pub(crate) async fn live_child(
    tx: &mut DbTx,
    account: Uuid,
    parent: Uuid,
    name_key: &str,
) -> Result<Option<(Uuid, NodeKind)>, DbError> {
    let row = sqlx::query(LIVE_CHILD)
        .bind(account)
        .bind(parent)
        .bind(name_key)
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| Ok((row.try_get(0)?, kind(&row, 1)?)))
        .transpose()
}

/// The child of `parent` holding `name_key`, live or else the latest tombstone: `(id, deleted)`.
pub(crate) async fn child_by_key(
    tx: &mut DbTx,
    account: Uuid,
    parent: Uuid,
    name_key: &str,
) -> Result<Option<(Uuid, bool)>, DbError> {
    let row = sqlx::query(CHILD_BY_KEY)
        .bind(account)
        .bind(parent)
        .bind(name_key)
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| Ok((row.try_get(0)?, row.try_get(1)?)))
        .transpose()
}

pub(crate) async fn live_file(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<LiveFileRow>, DbError> {
    let row = sqlx::query(LIVE_FILE)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let revision_id: Option<Uuid> = row.try_get(3)?;
    let revision = match revision_id {
        None => None,
        Some(revision_id) => {
            let content_hash: Vec<u8> = row.try_get(5)?;
            let client_mtime: Option<i64> = row.try_get(7)?;
            let view = RevisionView {
                id: revision_id,
                node_id: id,
                size: row.try_get(4)?,
                content_hash: file_hash(content_hash)?,
                blocklist: None,
                media_type: row.try_get(6)?,
                client_mtime: client_mtime.map(Timestamp::from_unix_micros),
                client_meta: row.try_get(8)?,
                created_at: Timestamp::from_unix_micros(row.try_get(9)?),
            };
            Some((view, row.try_get(10)?))
        }
    };
    Ok(Some(LiveFileRow {
        parent_id: row.try_get(0)?,
        kind: kind(&row, 1)?,
        deleted: row.try_get(2)?,
        revision,
    }))
}

/// At most `fetch` live children of `folder` whose key sorts after `after`.
pub(crate) async fn page(
    tx: &mut DbTx,
    account: Uuid,
    folder: Uuid,
    after: Option<&str>,
    fetch: i64,
) -> Result<Vec<ListedNode>, DbError> {
    let rows = sqlx::query(LISTING_PAGE_SQL)
        .bind(folder)
        .bind(account)
        .bind(after)
        .bind(fetch)
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(ListedNode {
                view: node_view(row.try_get(12)?, row)?,
                name_key: row.try_get(13)?,
            })
        })
        .collect()
}
