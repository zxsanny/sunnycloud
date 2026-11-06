use std::io;

use rusqlite::ErrorCode;
use sc_names::NameError;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// The file is not a usable index. Nothing was modified; the caller offers [`rebuild`](super::LocalIndexStore::rebuild).
    #[error("the local index is damaged")]
    IndexCorrupt,
    /// Written by a newer daemon. Nothing was modified; the caller refuses to start or offers a rebuild.
    #[error("the local index has schema version {found}, this daemon supports up to {supported}")]
    IndexNewerThanDaemon { found: u32, supported: u32 },
    #[error("local index i/o failed: {0}")]
    Io(#[from] io::Error),
    /// Any other database failure (constraint violation, misuse); not a sign of a damaged file.
    #[error("local index query failed: {0}")]
    Sqlite(rusqlite::Error),
    /// A case-insensitive pair cannot fold a path whose component is not a legal name.
    #[error("not a valid relative path: {0}")]
    InvalidPath(#[from] NameError),
}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::Error::{FromSqlConversionFailure, InvalidColumnType, SqliteFailure};
        match &error {
            SqliteFailure(failure, _) => match failure.code {
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => Self::IndexCorrupt,
                ErrorCode::SystemIoFailure
                | ErrorCode::CannotOpen
                | ErrorCode::DiskFull
                | ErrorCode::ReadOnly
                | ErrorCode::PermissionDenied => Self::Io(io::Error::other(error)),
                _ => Self::Sqlite(error),
            },
            // A stored value this daemon could not have written.
            FromSqlConversionFailure(..) | InvalidColumnType(..) => Self::IndexCorrupt,
            _ => Self::Sqlite(error),
        }
    }
}
