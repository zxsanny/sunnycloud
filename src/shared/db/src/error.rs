//! Typed database failures and the SQLSTATE table that produces them.

use thiserror::Error;

/// A database failure a caller can match. Callers keep a catch-all arm: new variants are a breaking change
/// only for exhaustive matches.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DbError {
    /// SQLSTATE 23505. `constraint` is the name the server reports.
    #[error("unique violation (constraint {constraint:?})")]
    UniqueViolation { constraint: Option<String> },
    /// SQLSTATE 23503, and 23001 (`restrict_violation`), which `ON DELETE RESTRICT` raises in its place.
    #[error("foreign key violation")]
    ForeignKeyViolation,
    /// SQLSTATE 23514.
    #[error("check violation")]
    CheckViolation,
    /// SQLSTATE 40001.
    #[error("serialization failure")]
    Serialization,
    /// The connection is lost, cannot be made or was terminated by the server (class 08, 57P01, 25P03).
    #[error("database unavailable")]
    Unavailable,
    /// `Db::connect` refused the connected role; the text names the failed check.
    #[error("wrong database role: {0}")]
    WrongRole(&'static str),
    /// Any other failure; the text carries the SQLSTATE and message for the log.
    #[error("database failure: {0}")]
    Other(String),
}

impl DbError {
    /// The one SQLSTATE table. `constraint` is the name from the error response.
    pub(crate) fn from_sqlstate(code: &str, constraint: Option<&str>, message: &str) -> DbError {
        match code {
            "23505" => DbError::UniqueViolation {
                constraint: constraint.map(str::to_owned),
            },
            "23503" | "23001" => DbError::ForeignKeyViolation,
            "23514" => DbError::CheckViolation,
            "40001" => DbError::Serialization,
            "57P01" | "25P03" => DbError::Unavailable,
            class08 if class08.starts_with("08") => DbError::Unavailable,
            other => DbError::Other(format!("SQLSTATE {other}: {message}")),
        }
    }
}

impl From<sqlx::Error> for DbError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::Database(database) => DbError::from_sqlstate(
                database.code().as_deref().unwrap_or(""),
                database.constraint(),
                database.message(),
            ),
            sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed => DbError::Unavailable,
            other => DbError::Other(other.to_string()),
        }
    }
}

/// Failure of `Db::run` and `Db::run_system`: the closure's own error (the transaction was rolled back) or a
/// database failure outside the closure (begin, scope, commit, rollback).
#[derive(Debug, Error)]
pub enum RunError<E> {
    #[error("transaction closure failed: {0}")]
    Closure(E),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlstate_table_maps_each_listed_code() {
        let cases = [
            ("23503", DbError::ForeignKeyViolation),
            ("23001", DbError::ForeignKeyViolation),
            ("23514", DbError::CheckViolation),
            ("40001", DbError::Serialization),
            ("57P01", DbError::Unavailable),
            ("25P03", DbError::Unavailable),
            ("08000", DbError::Unavailable),
            ("08003", DbError::Unavailable),
            ("08006", DbError::Unavailable),
            ("08P01", DbError::Unavailable),
        ];

        for (code, expected) in cases {
            let mapped = DbError::from_sqlstate(code, None, "message");

            assert_eq!(mapped, expected, "code {code}");
        }
    }

    #[test]
    fn unique_violation_carries_the_constraint_name() {
        let mapped = DbError::from_sqlstate("23505", Some("users_email_key"), "duplicate");

        assert_eq!(
            mapped,
            DbError::UniqueViolation {
                constraint: Some("users_email_key".into())
            }
        );
    }

    #[test]
    fn unlisted_codes_are_other_with_code_and_message() {
        for code in ["23502", "42P01", "57014", "22P02", "53300", "25P02", ""] {
            let mapped = DbError::from_sqlstate(code, None, "boom");

            assert_eq!(mapped, DbError::Other(format!("SQLSTATE {code}: boom")));
        }
    }

    #[test]
    fn pool_and_io_failures_are_unavailable() {
        let io = sqlx::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset));

        assert_eq!(DbError::from(io), DbError::Unavailable);
        assert_eq!(
            DbError::from(sqlx::Error::PoolTimedOut),
            DbError::Unavailable
        );
        assert_eq!(DbError::from(sqlx::Error::PoolClosed), DbError::Unavailable);
    }

    #[test]
    fn other_sqlx_errors_keep_their_text() {
        let mapped = DbError::from(sqlx::Error::RowNotFound);

        assert!(matches!(mapped, DbError::Other(text) if !text.is_empty()));
    }
}
