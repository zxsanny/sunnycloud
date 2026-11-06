use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::super::IndexError;

/// Forward-only; `PRAGMA user_version` is the number of files applied.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Opens the one writer connection. The file is read-checked before anything is written, so a damaged or newer
/// index is left byte for byte as it was.
pub(in crate::local_index) fn open_writer(file: &Path) -> Result<Connection, IndexError> {
    let mut conn = Connection::open(file)?;
    let check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(IndexError::IndexCorrupt);
    }
    let found: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if found > SCHEMA_VERSION {
        return Err(IndexError::IndexNewerThanDaemon {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    for (sql, version) in MIGRATIONS.iter().zip(1u32..).skip(found as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(conn)
}

/// A second connection for lookups; in WAL mode its reads never block the writer.
pub(in crate::local_index) fn open_reader(file: &Path) -> Result<Connection, IndexError> {
    Ok(Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}
