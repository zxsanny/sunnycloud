use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;
use sync_core_proto::Cursor;
use uuid::Uuid;

use super::path_key::path_key;
use super::repo::{entries, meta, migrate, pairs};
use super::{Entry, EntryState, IndexError, NewPair, Pair, PairState};
use crate::platform_fs::FileId;

pub const INDEX_FILE_NAME: &str = "local-index.sqlite3";

const CURSOR_KEY: &str = "cursor";

/// One writer connection (writes are serialised by its lock) and one read-only connection, so lookups never
/// wait for a write transaction. Every method is synchronous; async callers use a blocking task.
pub struct LocalIndexStore {
    writer: Mutex<Connection>,
    reader: Mutex<Connection>,
}

impl LocalIndexStore {
    /// Opens `data_dir/local-index.sqlite3`, creating the directory and the file when missing and applying
    /// pending migrations. A damaged or newer index is reported without being modified.
    pub fn open(data_dir: &Path) -> Result<Self, IndexError> {
        fs::create_dir_all(data_dir)?;
        let file = data_dir.join(INDEX_FILE_NAME);
        let writer = migrate::open_writer(&file)?;
        let reader = migrate::open_reader(&file)?;
        Ok(Self {
            writer: Mutex::new(writer),
            reader: Mutex::new(reader),
        })
    }

    /// Replaces the index with an empty one. Any open store on `data_dir` must be dropped first.
    pub fn rebuild(data_dir: &Path) -> Result<Self, IndexError> {
        let file = data_dir.join(INDEX_FILE_NAME);
        for path in [
            file.clone(),
            sidecar(&file, "-wal"),
            sidecar(&file, "-shm"),
            sidecar(&file, "-journal"),
        ] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Self::open(data_dir)
    }

    pub fn add_pair(&self, pair: &NewPair) -> Result<i64, IndexError> {
        self.write(|conn| Ok(pairs::insert(conn, pair)?))
    }

    pub fn get_pair(&self, id: i64) -> Result<Option<Pair>, IndexError> {
        self.read(|conn| Ok(pairs::get(conn, id)?))
    }

    /// Returns whether the pair exists.
    pub fn set_pair_state(
        &self,
        id: i64,
        state: PairState,
        hold_detail: Option<&str>,
    ) -> Result<bool, IndexError> {
        self.write(|conn| Ok(pairs::set_state(conn, id, state, hold_detail)?))
    }

    pub fn get_entry(&self, pair_id: i64, rel_path: &str) -> Result<Option<Entry>, IndexError> {
        self.read(|conn| {
            let key = path_key(rel_path, is_case_insensitive(conn, pair_id)?)?;
            Ok(entries::query(conn, entries::BY_PATH, &[&pair_id, &key])?
                .into_iter()
                .next())
        })
    }

    /// Inserts or updates every entry in one transaction. On a case-insensitive pair an entry whose path
    /// differs only in case from an existing one updates that entry, including its stored spelling.
    pub fn upsert_entries(&self, batch: &[Entry]) -> Result<(), IndexError> {
        self.write(|conn| {
            let tx = conn.transaction()?;
            let mut case_insensitive: HashMap<i64, bool> = HashMap::new();
            for entry in batch {
                let fold = match case_insensitive.get(&entry.pair_id) {
                    Some(fold) => *fold,
                    None => {
                        let fold = is_case_insensitive(&tx, entry.pair_id)?;
                        case_insensitive.insert(entry.pair_id, fold);
                        fold
                    }
                };
                entries::upsert(&tx, entry, &path_key(&entry.rel_path, fold)?)?;
            }
            Ok(tx.commit()?)
        })
    }

    pub fn entries_in_state(
        &self,
        pair_id: i64,
        state: EntryState,
    ) -> Result<Vec<Entry>, IndexError> {
        let sql = if state == EntryState::Synced {
            entries::SYNCED
        } else {
            entries::NOT_SYNCED
        };
        self.read(|conn| Ok(entries::query(conn, sql, &[&pair_id, &state.as_str()])?))
    }

    pub fn find_by_file_id(&self, pair_id: i64, file_id: FileId) -> Result<Vec<Entry>, IndexError> {
        let file_id = entries::file_id_text(file_id);
        self.read(|conn| {
            Ok(entries::query(
                conn,
                entries::BY_FILE_ID,
                &[&pair_id, &file_id],
            )?)
        })
    }

    pub fn find_by_node(&self, pair_id: i64, node_id: Uuid) -> Result<Vec<Entry>, IndexError> {
        self.read(|conn| {
            Ok(entries::query(
                conn,
                entries::BY_NODE,
                &[&pair_id, &node_id.as_bytes().as_slice()],
            )?)
        })
    }

    /// The single feed cursor of the account: one `meta` row, written atomically.
    pub fn set_cursor(&self, cursor: Cursor) -> Result<(), IndexError> {
        self.meta_set(CURSOR_KEY, &cursor.to_string())
    }

    pub fn cursor(&self) -> Result<Option<Cursor>, IndexError> {
        self.meta_get(CURSOR_KEY)?
            .map(|text| text.parse().map_err(|_| IndexError::IndexCorrupt))
            .transpose()
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>, IndexError> {
        self.read(|conn| Ok(meta::get(conn, key)?))
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<(), IndexError> {
        self.write(|conn| Ok(meta::set(conn, key, value)?))
    }

    pub(super) fn read<T>(
        &self,
        op: impl FnOnce(&mut Connection) -> Result<T, IndexError>,
    ) -> Result<T, IndexError> {
        op(&mut lock(&self.reader))
    }

    pub(super) fn write<T>(
        &self,
        op: impl FnOnce(&mut Connection) -> Result<T, IndexError>,
    ) -> Result<T, IndexError> {
        op(&mut lock(&self.writer))
    }
}

fn is_case_insensitive(conn: &Connection, pair_id: i64) -> Result<bool, IndexError> {
    Ok(pairs::case_insensitive(conn, pair_id)?.unwrap_or(false))
}

/// Every operation is one statement or one transaction, so a panic elsewhere cannot leave a half-written state.
fn lock(connection: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    connection.lock().unwrap_or_else(PoisonError::into_inner)
}

fn sidecar(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
