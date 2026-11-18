//! The checkpoint database (17 §4): app-private SQLite in WAL mode with one connection, so the crate is its only
//! writer. The file is created and migrated on first use, never at construction. Every write is one transaction.

mod asset_repo;
mod repo;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rusqlite::{Connection, OpenFlags};
use thiserror::Error;

pub(crate) use asset_repo::{Acknowledged, AssetStore, PendingAsset};
pub use repo::SqliteCheckpointStore;

/// Forward migrations, oldest first: entry `n` takes a database from version `n` to `n + 1`.
const MIGRATIONS: &[&str] = &[include_str!("schema.sql")];

/// The schema version this build writes.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

const BUSY_TIMEOUT_MS: u32 = 5_000;

#[derive(Debug, Error)]
pub enum DbError {
    /// The file was written by a newer build; it is left unchanged.
    #[error(
        "checkpoint database is at schema version {found}, this build supports up to {supported}"
    )]
    NewerSchema { found: u32, supported: u32 },
    #[error("checkpoint database failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A stored value broke an invariant of its table.
    #[error("checkpoint database holds a malformed {0}")]
    Malformed(&'static str),
}

/// The lazily opened checkpoint database of one app-private path.
#[derive(Debug)]
pub struct CheckpointDb {
    path: PathBuf,
    migrations: &'static [&'static str],
    connection: Mutex<Option<Connection>>,
}

impl CheckpointDb {
    /// Remembers `path`; nothing is created or opened until the first [`CheckpointDb::write`] or [`CheckpointDb::read`].
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_migrations(path, MIGRATIONS)
    }

    pub(crate) fn with_migrations(
        path: impl Into<PathBuf>,
        migrations: &'static [&'static str],
    ) -> Self {
        Self {
            path: path.into(),
            migrations,
            connection: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `step` in one transaction: it commits when `step` answers `Ok` and rolls back otherwise.
    pub fn write<T>(
        &self,
        step: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let value = step(&transaction)?;
            transaction.commit()?;
            Ok(value)
        })
    }

    pub fn read<T>(
        &self,
        query: impl FnOnce(&Connection) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        self.with_connection(|connection| query(connection))
    }

    /// A panic inside `step` rolls its transaction back when the transaction drops, so a poisoned lock holds a sound
    /// connection.
    fn with_connection<T>(
        &self,
        run: impl FnOnce(&mut Connection) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let mut slot = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let connection = match &mut *slot {
            Some(connection) => connection,
            empty => empty.insert(self.open()?),
        };
        run(connection)
    }

    /// Opens the file, refuses a newer schema before touching it, then sets the pragmas and migrates forward.
    fn open(&self) -> Result<Connection, DbError> {
        let mut connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let supported = self.migrations.len() as u32;
        let found: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if found > supported {
            return Err(DbError::NewerSchema { found, supported });
        }
        connection.busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        for version in found..supported {
            let transaction = connection.transaction()?;
            transaction.execute_batch(self.migrations[version as usize])?;
            transaction.pragma_update(None, "user_version", version + 1)?;
            transaction.commit()?;
        }
        Ok(connection)
    }
}

#[cfg(test)]
mod tests;
