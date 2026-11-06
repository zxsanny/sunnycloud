use contracts::QuotaLedgerError;
use sc_db::{DbError, RunError};
use thiserror::Error;
use uuid::Uuid;

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

/// The source of an unexpected ledger failure that is not a database failure.
#[derive(Debug, Error)]
pub enum LedgerFault {
    /// The account lifecycle hook creates the quota row with the account, so a missing row is a bug upstream.
    #[error("quota row missing for account {account}")]
    MissingRow { account: Uuid },
}
