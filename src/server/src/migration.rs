//! `migrate [run]`, `migrate status` and `migrate revert` (E-22/c07): one call on helper 07's `Migrator`, its
//! result as the outcome, its refusals as exit 2. Locking, transactions and checksums are the migrator's.

use sc_db::{Db, DbError, MigrationError, MigrationReport, MigrationStatus, Migrator};
use sc_error::ErrorCode;

use crate::command_config;
use crate::commands::{Command, MigrateAction, MigrateArgs};
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::{Class, Outcome};

pub(crate) async fn run(
    command: &Command,
    args: &MigrateArgs,
    services: &ServiceContext,
) -> Result<Outcome, CliError> {
    let (config, role) = command_config::database(command.config_check(), services).await?;
    let db = Db::connect(&config, role).await.map_err(database_failure)?;
    match args.action {
        None | Some(MigrateAction::Run) => Migrator::run_pending(&db)
            .await
            .map(applied)
            .map_err(refusal),
        Some(MigrateAction::Status) => Migrator::status(&db)
            .await
            .map(status)
            .map_err(database_failure),
        Some(MigrateAction::Revert { steps, .. }) => Migrator::revert(&db, steps)
            .await
            .map(reverted)
            .map_err(refusal),
    }
}

fn applied(report: MigrationReport) -> Outcome {
    Outcome::success().field("applied", report.applied)
}

fn reverted(report: MigrationReport) -> Outcome {
    Outcome::success().field("reverted", report.reverted)
}

/// A dirty version, a checksum mismatch or an unknown-newer version is a finding (exit 2, owner decision
/// 2026-11-12), so a script that reads only the exit code sees every state `migrate` would refuse.
fn status(status: MigrationStatus) -> Outcome {
    let class = if status.unknown_newer.is_empty()
        && status.dirty.is_none()
        && status.checksum_mismatch.is_empty()
    {
        Class::Success
    } else {
        Class::Findings
    };
    Outcome::new(class)
        .field("applied", status.applied)
        .field("pending", status.pending)
        .field("dirty", status.dirty)
        .field("unknown-newer", status.unknown_newer)
        .field("checksum-mismatch", status.checksum_mismatch)
}

/// Exit 2 with the typed name and the version in the text. The codes are this command's until helper 02's table
/// lists them (contract open point 3).
fn refusal(error: MigrationError) -> CliError {
    let (code, kind) = match &error {
        MigrationError::MigrationFailed { .. } => ("migration_failed", "MigrationFailed"),
        MigrationError::ChecksumMismatch { .. } => ("checksum_mismatch", "ChecksumMismatch"),
        MigrationError::UnknownNewer { .. } => ("unknown_newer", "UnknownNewer"),
        MigrationError::IrreversibleMigration { .. } => {
            ("irreversible_migration", "IrreversibleMigration")
        }
        MigrationError::Dirty { .. } => ("migration_dirty", "Dirty"),
        MigrationError::WrongRole => ("wrong_role", "WrongRole"),
        MigrationError::Db(error) => return database_failure(error.clone()),
    };
    CliError::Operator {
        code,
        detail: format!("{kind}: {error}"),
        exit: Exit::Failed,
    }
}

fn database_failure(error: DbError) -> CliError {
    let code = match error {
        DbError::Unavailable => ErrorCode::ServiceUnavailable,
        _ => ErrorCode::Internal,
    };
    CliError::Operator {
        code: code.as_str(),
        detail: format!("database: {error}"),
        exit: Exit::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSION: i64 = 20_261_105_090_000;

    fn refused(error: MigrationError) -> (Exit, &'static str, String) {
        let error = refusal(error);
        (error.exit(), error.code(), error.to_string())
    }

    #[test]
    fn every_migration_refusal_exits_2_with_its_code_name_and_version() {
        // Arrange
        let cases = [
            (
                MigrationError::MigrationFailed {
                    version: VERSION,
                    error: DbError::Other("SQLSTATE 22012: division by zero".into()),
                },
                "migration_failed",
                "MigrationFailed",
            ),
            (
                MigrationError::ChecksumMismatch { version: VERSION },
                "checksum_mismatch",
                "ChecksumMismatch",
            ),
            (
                MigrationError::UnknownNewer { version: VERSION },
                "unknown_newer",
                "UnknownNewer",
            ),
            (
                MigrationError::IrreversibleMigration { version: VERSION },
                "irreversible_migration",
                "IrreversibleMigration",
            ),
            (
                MigrationError::Dirty { version: VERSION },
                "migration_dirty",
                "Dirty",
            ),
        ];

        for (error, code, kind) in cases {
            // Act
            let (exit, actual_code, text) = refused(error);

            // Assert
            assert_eq!(exit, Exit::Failed, "{kind}");
            assert_eq!(actual_code, code, "{kind}");
            assert!(text.starts_with(kind), "{text}");
            assert!(text.contains(&VERSION.to_string()), "{text}");
        }
    }

    #[test]
    fn a_failed_migration_names_the_database_error() {
        // Act
        let (_, _, text) = refused(MigrationError::MigrationFailed {
            version: VERSION,
            error: DbError::Other("SQLSTATE 22012: division by zero".into()),
        });

        // Assert
        assert!(text.contains("division by zero"), "{text}");
    }

    #[test]
    fn database_failures_exit_2_and_an_unreachable_database_is_unavailable() {
        // Act
        let unavailable = refusal(MigrationError::Db(DbError::Unavailable));
        let wrong_role = refusal(MigrationError::WrongRole);

        // Assert
        assert_eq!(
            (unavailable.exit(), unavailable.code()),
            (Exit::Failed, "service_unavailable")
        );
        assert_eq!(
            (wrong_role.exit(), wrong_role.code()),
            (Exit::Failed, "wrong_role")
        );
    }

    fn migration_status(dirty: Option<i64>, unknown_newer: Vec<i64>) -> MigrationStatus {
        MigrationStatus {
            applied: vec![VERSION],
            pending: Vec::new(),
            dirty,
            unknown_newer,
            checksum_mismatch: Vec::new(),
        }
    }

    #[test]
    fn status_with_an_unknown_newer_version_is_a_finding() {
        // Act
        let outcome = status(migration_status(None, vec![99_990_101_000_000]));

        // Assert
        assert_eq!(outcome.exit(), Exit::Failed);
        assert_eq!(
            outcome.data["unknown-newer"],
            serde_json::json!([99_990_101_000_000_i64])
        );
    }

    #[test]
    fn status_with_a_dirty_version_is_a_finding() {
        // Act
        let outcome = status(migration_status(Some(VERSION), Vec::new()));

        // Assert
        assert_eq!(outcome.exit(), Exit::Failed);
        assert_eq!(outcome.data["dirty"], serde_json::json!(VERSION));
        assert_eq!(outcome.data["applied"], serde_json::json!([VERSION]));
    }
}
