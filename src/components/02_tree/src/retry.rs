use contracts::TreeServiceError;
use sc_db::{Db, DbError, DbTx, RunError, Scope};

use crate::error::TreeError;

/// SQLSTATE of a deadlock; `sc_db` has no variant for it, and keeps the code at the start of the text of `Other`.
const DEADLOCK_PREFIX: &str = "SQLSTATE 40P01";

/// The error of a unit of work run by [`run_retrying`]: it can carry a database failure, and the retry rule
/// reads that failure to decide.
pub trait TxFailure {
    fn from_db(error: DbError) -> Self;
    fn db_error(&self) -> Option<&DbError>;
}

impl TxFailure for TreeError {
    fn from_db(error: DbError) -> Self {
        TreeError::Database(error)
    }

    fn db_error(&self) -> Option<&DbError> {
        match self {
            TreeError::Database(error) => Some(error),
            _ => None,
        }
    }
}

/// A database failure travels as the boxed `DbError` of `Unexpected`, also when a port (the feed stamp) returned
/// it, so a deadlock in any statement of the unit of work is seen.
impl TxFailure for TreeServiceError {
    fn from_db(error: DbError) -> Self {
        TreeServiceError::Unexpected(Box::new(error))
    }

    fn db_error(&self) -> Option<&DbError> {
        match self {
            TreeServiceError::Unexpected(fault) => fault.downcast_ref::<DbError>(),
            _ => None,
        }
    }
}

/// Runs `f` in one transaction scoped to `scope`. When the database aborts the transaction for a deadlock or a
/// serialization failure, whether inside `f` or at commit, the whole unit of work runs once more in a new
/// transaction. A second abort is returned as it is: the unexpected tier.
pub async fn run_retrying<T, E: TxFailure>(
    db: &Db,
    scope: Scope,
    f: impl AsyncFn(&mut DbTx) -> Result<T, E>,
) -> Result<T, E> {
    match run_once(db, scope, &f).await {
        Err(error) if aborted(&error) => run_once(db, scope, &f).await,
        outcome => outcome,
    }
}

async fn run_once<T, E: TxFailure>(
    db: &Db,
    scope: Scope,
    f: &impl AsyncFn(&mut DbTx) -> Result<T, E>,
) -> Result<T, E> {
    db.run(scope, async |tx| f(tx).await)
        .await
        .map_err(|error| match error {
            RunError::Closure(error) => error,
            RunError::Db(error) => E::from_db(error),
        })
}

fn aborted(error: &impl TxFailure) -> bool {
    match error.db_error() {
        Some(DbError::Serialization) => true,
        Some(DbError::Other(text)) => text.starts_with(DEADLOCK_PREFIX),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deadlock_inside_a_service_failure_is_an_abort() {
        // Arrange
        let deadlock = TreeServiceError::from_db(DbError::Other(format!(
            "{DEADLOCK_PREFIX}: deadlock detected"
        )));

        // Act
        let retried = aborted(&deadlock);

        // Assert
        assert!(retried);
    }

    #[test]
    fn a_business_or_foreign_service_failure_is_not_an_abort() {
        // Arrange
        let failures = [
            TreeServiceError::NameConflict,
            TreeServiceError::Unexpected(Box::new(std::fmt::Error)),
            TreeServiceError::from_db(DbError::CheckViolation),
        ];

        // Act
        let retried = failures.iter().map(aborted).collect::<Vec<_>>();

        // Assert
        assert_eq!(retried, [false, false, false]);
    }
}
