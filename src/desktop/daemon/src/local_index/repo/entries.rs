use rusqlite::types::Type;
use rusqlite::{Connection, Row, ToSql, params};
use sync_core_proto::FileHash;
use uuid::Uuid;

use super::super::{Entry, EntryState};
use crate::platform_fs::FileId;

macro_rules! select_entry {
    ($($clause:literal),+) => {
        concat!(
            "SELECT pair_id, COALESCE(disk_path, rel_path), node_id, revision_id, content_hash, size, mtime_ns, \
             file_id, state, settle_until, escaped_from FROM entry ",
            $($clause),+
        )
    };
}

pub(in crate::local_index) const BY_PATH: &str =
    select_entry!("WHERE pair_id = ?1 AND rel_path = ?2");
// Without table statistics SQLite prefers the primary key (a scan of the whole pair) over the narrower secondary
// indexes, so the three queries below name their index; SQLite then fails the statement if it cannot use it.
pub(in crate::local_index) const BY_FILE_ID: &str =
    select_entry!("INDEXED BY entry_file_id WHERE pair_id = ?1 AND file_id = ?2");
pub(in crate::local_index) const BY_NODE: &str =
    select_entry!("INDEXED BY entry_node_id WHERE pair_id = ?1 AND node_id = ?2");
/// The extra `state <> 'synced'` term is what lets SQLite use the partial index.
pub(in crate::local_index) const NOT_SYNCED: &str = select_entry!(
    "INDEXED BY entry_not_synced WHERE pair_id = ?1 AND state <> 'synced' AND state = ?2"
);
pub(in crate::local_index) const SYNCED: &str = select_entry!("WHERE pair_id = ?1 AND state = ?2");
/// Lookup keys that start with `<dir>/` are exactly `[<dir>/, <dir>0)`: `0` is the byte after `/`.
pub(in crate::local_index) const UNDER: &str =
    select_entry!("WHERE pair_id = ?1 AND rel_path >= ?2 AND rel_path < ?3");

const UPSERT: &str = "INSERT INTO entry (pair_id, rel_path, disk_path, node_id, revision_id, content_hash, size, \
    mtime_ns, file_id, state, settle_until, escaped_from) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
    ON CONFLICT (pair_id, rel_path) DO UPDATE SET disk_path = excluded.disk_path, node_id = excluded.node_id, \
    revision_id = excluded.revision_id, content_hash = excluded.content_hash, size = excluded.size, \
    mtime_ns = excluded.mtime_ns, file_id = excluded.file_id, state = excluded.state, \
    settle_until = excluded.settle_until, escaped_from = excluded.escaped_from";

/// `key` is the lookup form of the entry's path (see `path_key`).
pub(in crate::local_index) fn upsert(
    conn: &Connection,
    entry: &Entry,
    key: &str,
) -> rusqlite::Result<()> {
    let disk_path = (entry.rel_path != key).then_some(entry.rel_path.as_str());
    conn.prepare_cached(UPSERT)?.execute(params![
        entry.pair_id,
        key,
        disk_path,
        entry.node_id.as_ref().map(|id| id.as_bytes().as_slice()),
        entry
            .revision_id
            .as_ref()
            .map(|id| id.as_bytes().as_slice()),
        entry
            .content_hash
            .as_ref()
            .map(|hash| hash.as_bytes().as_slice()),
        entry.size.cast_signed(),
        entry.mtime_ns,
        file_id_text(entry.file_id),
        entry.state.as_str(),
        entry.settle_until,
        entry.escaped_from,
    ])?;
    Ok(())
}

pub(in crate::local_index) fn query(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> rusqlite::Result<Vec<Entry>> {
    conn.prepare_cached(sql)?
        .query_map(params, from_row)?
        .collect()
}

pub(in crate::local_index) fn any(conn: &Connection, pair_id: i64) -> rusqlite::Result<bool> {
    conn.prepare_cached("SELECT EXISTS (SELECT 1 FROM entry WHERE pair_id = ?1)")?
        .query_row([pair_id], |row| row.get(0))
}

pub(in crate::local_index) fn file_id_text(id: FileId) -> String {
    format!("{}:{}", id.volume, id.index)
}

fn parse_file_id(text: &str) -> Option<FileId> {
    let (volume, index) = text.split_once(':')?;
    Some(FileId {
        volume: volume.parse().ok()?,
        index: index.parse().ok()?,
    })
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
    let file_id: String = row.get(7)?;
    let state: String = row.get(8)?;
    Ok(Entry {
        pair_id: row.get(0)?,
        rel_path: row.get(1)?,
        node_id: optional_uuid(row, 2)?,
        revision_id: optional_uuid(row, 3)?,
        content_hash: optional_hash(row, 4)?,
        size: row.get::<_, i64>(5)?.cast_unsigned(),
        mtime_ns: row.get(6)?,
        file_id: parse_file_id(&file_id).ok_or_else(|| conversion_failure(7, "file id"))?,
        state: EntryState::parse(&state).ok_or_else(|| conversion_failure(8, "entry state"))?,
        settle_until: row.get(9)?,
        escaped_from: row.get(10)?,
    })
}

fn optional_uuid(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<Uuid>> {
    row.get::<_, Option<Vec<u8>>>(column)?
        .map(|bytes| uuid_from_blob(&bytes, column))
        .transpose()
}

fn optional_hash(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<FileHash>> {
    row.get::<_, Option<Vec<u8>>>(column)?
        .map(|bytes| {
            <[u8; 32]>::try_from(bytes.as_slice())
                .map(FileHash::from_bytes)
                .map_err(|_| conversion_failure(column, "content hash"))
        })
        .transpose()
}

pub(super) fn uuid_from_blob(bytes: &[u8], column: usize) -> rusqlite::Result<Uuid> {
    Uuid::from_slice(bytes).map_err(|_| conversion_failure(column, "uuid"))
}

pub(super) fn conversion_failure(column: usize, what: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Blob, what.into())
}
