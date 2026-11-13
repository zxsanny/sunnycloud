//! Statements of `publish`: the file node and its revisions. Every statement names `account_id`, so a foreign id
//! behaves like a random one (row-level security is the second line).

use contracts::{NewRevision, NodeKind};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

use crate::node_repo::{CurrentContent, current_content};

/// The node an `Existing` target names, with the content hash of its current revision.
pub(crate) struct FileRow {
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: NodeKind,
    pub deleted: bool,
    pub current_revision_id: Option<Uuid>,
    pub current_hash: Option<Vec<u8>>,
    pub current: Option<CurrentContent>,
}

impl FileRow {
    /// The current revision while it counts in usage: the file's own node is live.
    pub fn counted(&self) -> Option<&CurrentContent> {
        self.current.as_ref().filter(|_| !self.deleted)
    }
}

/// A file node about to be inserted with `revision_id` as its current revision: a new file's first revision, or
/// the superseded revision a conflict copy keeps.
pub(crate) struct NewFile<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub parent_id: Uuid,
    pub name: &'a str,
    pub name_key: &'a str,
    pub revision_id: Uuid,
    pub now: Timestamp,
}

const FILE_FOR_UPDATE: &str = "SELECT n.kind, n.deleted_at IS NOT NULL, n.current_revision_id, r.content_hash, \
    n.parent_id, n.name, r.blob_id, r.size \
    FROM nodes n LEFT JOIN revisions r ON r.id = n.current_revision_id AND r.account_id = n.account_id \
    WHERE n.id = $1 AND n.account_id = $2 FOR UPDATE OF n";

// The pointer to the revision is checked at COMMIT (deferred), so the node can name a revision inserted after it.
// A taken name inserts nothing instead of failing, also when the other row is not committed yet (the insert waits
// for it), so the entry's savepoint is never aborted by the name index.
const INSERT_FILE: &str = "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, \
    name_key_version, current_revision_id, created_at, updated_at) \
    VALUES ($1, $2, $3, 'file', $4, $5, $6, $7, \
    'epoch'::timestamptz + $8::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $8::bigint * interval '1 microsecond') \
    ON CONFLICT (parent_id, name_key) WHERE deleted_at IS NULL DO NOTHING";

const INSERT_REVISION: &str = "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, \
    blocklist, media_type, client_mtime, client_meta, created_at) \
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, \
    'epoch'::timestamptz + $9::bigint * interval '1 microsecond', $10::jsonb, \
    'epoch'::timestamptz + $11::bigint * interval '1 microsecond')";

const SET_CURRENT: &str = "UPDATE nodes SET current_revision_id = $3, \
    updated_at = 'epoch'::timestamptz + $4::bigint * interval '1 microsecond' \
    WHERE id = $1 AND account_id = $2";

pub(crate) async fn lock_file(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
) -> Result<Option<FileRow>, DbError> {
    let row = sqlx::query(FILE_FOR_UPDATE)
        .bind(id)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| {
        let kind = match row.try_get::<&str, _>(0)? {
            "file" => NodeKind::File,
            "folder" => NodeKind::Folder,
            other => return Err(DbError::Other(format!("unknown node kind {other:?}"))),
        };
        Ok(FileRow {
            kind,
            deleted: row.try_get(1)?,
            current_revision_id: row.try_get(2)?,
            current_hash: row.try_get(3)?,
            parent_id: row.try_get(4)?,
            name: row.try_get(5)?,
            current: current_content(&row, 6)?,
        })
    })
    .transpose()
}

/// `false` when the name is taken in the folder: nothing was inserted.
pub(crate) async fn insert_file(tx: &mut DbTx, file: &NewFile<'_>) -> Result<bool, DbError> {
    let done = sqlx::query(INSERT_FILE)
        .bind(file.id)
        .bind(file.account)
        .bind(file.parent_id)
        .bind(file.name)
        .bind(file.name_key)
        .bind(sc_names::NAME_KEY_VERSION)
        .bind(file.revision_id)
        .bind(file.now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

pub(crate) async fn insert_revision(
    tx: &mut DbTx,
    account: Uuid,
    node: Uuid,
    revision: &NewRevision,
    now: Timestamp,
) -> Result<(), DbError> {
    let blocklist: Vec<u8> = revision
        .blocklist
        .iter()
        .flat_map(|hash| *hash.as_bytes())
        .collect();
    sqlx::query(INSERT_REVISION)
        .bind(revision.revision_id)
        .bind(account)
        .bind(node)
        .bind(revision.blob_id)
        .bind(revision.size)
        .bind(revision.content_hash.as_bytes().as_slice())
        .bind(blocklist)
        .bind(revision.media_type.as_deref())
        .bind(revision.client_mtime.map(Timestamp::as_unix_micros))
        .bind(revision.client_meta.as_deref())
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn set_current(
    tx: &mut DbTx,
    account: Uuid,
    node: Uuid,
    revision: Uuid,
    now: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(SET_CURRENT)
        .bind(node)
        .bind(account)
        .bind(revision)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}
