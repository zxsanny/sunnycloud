//! Statements of the revision reads (`TreeReader`), the device query (`TreeService::find_revisions`) and the live
//! usage sum (`LiveUsageSource`). Every account-scoped statement names `account_id`, so a foreign id behaves like a
//! random one (row-level security is the second line). `DESCRIBE_BLOBS` serves `doctor` in system scope: its
//! entry by blob id is the one lookup that does not know the account, and every join after it names `account_id`.

use contracts::{RevisionMatch, RevisionRef, RevisionView};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::{FileHash, Hash32};
use sqlx::Row;
use uuid::Uuid;

use crate::node_repo::file_hash;

/// The tree position of one revision's blob: `path` is `None` when the file or a folder above it is tombstoned.
pub(crate) struct BlobPlace {
    pub blob_id: Uuid,
    pub path: Option<String>,
}

const REVISION: &str = "SELECT node_id, size, content_hash, blocklist, media_type, \
    (extract(epoch FROM client_mtime) * 1000000)::bigint, client_meta::text, \
    (extract(epoch FROM created_at) * 1000000)::bigint \
    FROM revisions WHERE id = $1 AND account_id = $2";

// A revision row exists until the retention purge or the account erase collects it.
const REVISION_EXISTS: &str =
    "SELECT EXISTS (SELECT 1 FROM revisions WHERE id = $1 AND account_id = $2)";

/// The batched hash lookup (AC-31, AC-46): `revisions_account_id_content_hash_idx` for the hashes, then the node by
/// primary key; only a revision that is its live node's current one matches. Public for the plan test only.
#[doc(hidden)]
pub const LOOKUP_REVISIONS_SQL: &str = "SELECT r.content_hash, r.node_id, r.id, r.client_meta::text \
    FROM revisions r JOIN nodes n ON n.id = r.node_id AND n.account_id = r.account_id \
    WHERE r.account_id = $1 AND r.content_hash = ANY($2::bytea[]) \
    AND n.current_revision_id = r.id AND n.deleted_at IS NULL \
    ORDER BY r.id";

/// The revisions of one device (AC-46), read through `revisions_client_device_idx` and paged in id order. Public
/// for the plan test only.
#[doc(hidden)]
pub const DEVICE_REVISIONS_SQL: &str = "SELECT id, node_id FROM revisions \
    WHERE account_id = $1 AND client_meta IS NOT NULL AND client_meta ->> 'device' = $2 \
    ORDER BY id OFFSET $3 LIMIT $4";

const ALL_REVISIONS: &str = "SELECT id, node_id FROM revisions WHERE account_id = $1 \
    ORDER BY id OFFSET $2 LIMIT $3";

// Walks up from every revision's node at once; the tree is acyclic, so each walk ends at the root (no name). The
// path joins the names below the root, farthest first; any tombstone on the way marks the blob deleted.
const DESCRIBE_BLOBS: &str = "WITH RECURSIVE chain (blob_id, account_id, id, parent_id, name, deleted, depth) AS ( \
    SELECT r.blob_id, n.account_id, n.id, n.parent_id, n.name, n.deleted_at IS NOT NULL, 0 \
    FROM revisions r JOIN nodes n ON n.id = r.node_id AND n.account_id = r.account_id \
    WHERE r.blob_id = ANY($1) \
    UNION ALL \
    SELECT c.blob_id, n.account_id, n.id, n.parent_id, n.name, n.deleted_at IS NOT NULL, c.depth + 1 \
    FROM chain c JOIN nodes n ON n.id = c.parent_id AND n.account_id = c.account_id) \
    SELECT blob_id, bool_or(deleted), \
    string_agg(name, '/' ORDER BY depth DESC) FILTER (WHERE parent_id IS NOT NULL) \
    FROM chain GROUP BY blob_id";

const LIVE_BYTES: &str = "SELECT COALESCE(sum(r.size), 0)::bigint \
    FROM nodes n JOIN revisions r ON r.id = n.current_revision_id AND r.account_id = n.account_id \
    WHERE n.account_id = $1 AND n.kind = 'file' AND n.deleted_at IS NULL";

pub(crate) async fn revision(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<RevisionView>, DbError> {
    let row = sqlx::query(REVISION)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let blocklist: Vec<u8> = row.try_get(3)?;
    let client_mtime: Option<i64> = row.try_get(5)?;
    Ok(Some(RevisionView {
        id,
        node_id: row.try_get(0)?,
        size: row.try_get(1)?,
        content_hash: file_hash(row.try_get(2)?)?,
        blocklist: Some(blocks(&blocklist)?),
        media_type: row.try_get(4)?,
        client_mtime: client_mtime.map(Timestamp::from_unix_micros),
        client_meta: row.try_get(6)?,
        created_at: Timestamp::from_unix_micros(row.try_get(7)?),
    }))
}

pub(crate) async fn exists(tx: &mut DbTx, account: Uuid, id: Uuid) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(REVISION_EXISTS)
        .bind(id)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

/// The current revisions of live files whose content hash is one of `hashes`, in one statement.
pub(crate) async fn lookup(
    tx: &mut DbTx,
    account: Uuid,
    hashes: &[FileHash],
) -> Result<Vec<RevisionMatch>, DbError> {
    let hashes: Vec<&[u8]> = hashes.iter().map(|hash| &hash.as_bytes()[..]).collect();
    let rows = sqlx::query(LOOKUP_REVISIONS_SQL)
        .bind(account)
        .bind(&hashes)
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(RevisionMatch {
                hash: file_hash(row.try_get(0)?)?,
                node_id: row.try_get(1)?,
                revision_id: row.try_get(2)?,
                client_meta: row.try_get(3)?,
            })
        })
        .collect()
}

/// At most `limit` revisions after the first `offset` in id order, of one device or (`None`) of every device.
pub(crate) async fn page(
    tx: &mut DbTx,
    account: Uuid,
    device: Option<&str>,
    offset: i64,
    limit: i64,
) -> Result<Vec<RevisionRef>, DbError> {
    let query = match device {
        Some(device) => sqlx::query(DEVICE_REVISIONS_SQL).bind(account).bind(device),
        None => sqlx::query(ALL_REVISIONS).bind(account),
    };
    let rows = query.bind(offset).bind(limit).fetch_all(tx.conn()).await?;
    rows.iter()
        .map(|row| {
            Ok(RevisionRef {
                revision_id: row.try_get(0)?,
                node_id: row.try_get(1)?,
            })
        })
        .collect()
}

/// One entry per blob id that a revision row holds; an unknown id has none.
pub(crate) async fn describe_blobs(
    tx: &mut DbTx,
    blob_ids: &[Uuid],
) -> Result<Vec<BlobPlace>, DbError> {
    let rows = sqlx::query(DESCRIBE_BLOBS)
        .bind(blob_ids)
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            let deleted: bool = row.try_get(1)?;
            let path: Option<String> = row.try_get(2)?;
            Ok(BlobPlace {
                blob_id: row.try_get(0)?,
                path: path.filter(|_| !deleted),
            })
        })
        .collect()
}

pub(crate) async fn live_bytes(tx: &mut DbTx, account: Uuid) -> Result<i64, DbError> {
    Ok(sqlx::query_scalar(LIVE_BYTES)
        .bind(account)
        .fetch_one(tx.conn())
        .await?)
}

/// The stored blocklist: 32-byte block hashes back to back (`revisions_blocklist_check`).
fn blocks(bytes: &[u8]) -> Result<Vec<Hash32>, DbError> {
    let (chunks, rest) = bytes.as_chunks::<32>();
    if !rest.is_empty() {
        return Err(DbError::Other(format!(
            "blocklist of {} bytes is not a multiple of 32",
            bytes.len()
        )));
    }
    Ok(chunks
        .iter()
        .map(|chunk| Hash32::from_bytes(*chunk))
        .collect())
}
