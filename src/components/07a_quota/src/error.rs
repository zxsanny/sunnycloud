use contracts::QuotaLedgerError;
use sc_db::{DbError, RunError};

/// The database failure behind a `Db::run` result, whether the closure or the transaction raised it.
pub fn db_error(error: RunError<DbError>) -> DbError {
    match error {
        RunError::Closure(error) | RunError::Db(error) => error,
    }
}

/// A CHECK violation on `quota` is a counter that would go negative. Every other database failure is the
/// unexpected tier: its text goes to the log through the source, never into the message.
pub fn ledger_error(error: DbError) -> QuotaLedgerError {
    match error {
        DbError::CheckViolation => QuotaLedgerError::InvariantViolation,
        other => QuotaLedgerError::Unexpected(Box::new(other)),
    }
}
