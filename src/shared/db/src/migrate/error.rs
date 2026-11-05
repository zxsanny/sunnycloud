use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::error::DbError;

/// Why a migration run, revert or version check failed. Callers keep a catch-all arm.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrationError {
    /// The migration's own transaction failed and was rolled back; earlier migrations stay applied.
    #[error("migration {version} failed: {error}")]
    MigrationFailed { version: i64, error: DbError },
    /// An applied migration file was changed afterwards.
    #[error("migration {version} was applied with different file content")]
    ChecksumMismatch { version: i64 },
    /// The database has applied versions this binary does not know (the newest is named).
    #[error("the database has migration {version} applied, which this binary does not know")]
    UnknownNewer { version: i64 },
    /// The `down` of this migration raises `irreversible: restore from backup`.
    #[error("migration {version} cannot be reverted: restore from backup")]
    IrreversibleMigration { version: i64 },
    /// The bookkeeping table records this migration as unsuccessful.
    #[error("migration {version} is recorded as unsuccessful")]
    Dirty { version: i64 },
    /// Only a [`crate::DbRole::Migrator`] pool changes the schema.
    #[error("only the migrator role can run or revert migrations")]
    WrongRole,
    #[error(transparent)]
    Db(#[from] DbError),
}

const IRREVERSIBLE_PREFIX: &str = "irreversible:";

impl MigrationError {
    /// `reverting` tells a `down` that raises the irreversible marker from any other failure of the file.
    pub(crate) fn from_migrate(error: MigrateError, reverting: bool) -> Self {
        match error {
            MigrateError::ExecuteMigration(error, version) => {
                if reverting && raised_irreversible(&error) {
                    MigrationError::IrreversibleMigration { version }
                } else {
                    MigrationError::MigrationFailed {
                        version,
                        error: error.into(),
                    }
                }
            }
            MigrateError::Execute(error) => MigrationError::Db(error.into()),
            MigrateError::Dirty(version) => MigrationError::Dirty { version },
            other => MigrationError::Db(DbError::Other(other.to_string())),
        }
    }
}

fn raised_irreversible(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database)
        if database.code().as_deref() == Some("P0001")
            && database.message().starts_with(IRREVERSIBLE_PREFIX))
}
