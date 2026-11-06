use sc_db::{Db, DbError, DbTx, RunError, Scope};

use crate::error::TreeError;

/// SQLSTATE of a deadlock; `sc_db` has no variant for it, and keeps the code at the start of the text of `Other`.
const DEADLOCK_PREFIX: &str = "SQLSTATE 40P01";

/// Runs `f` in one transaction scoped to `scope`. When the database aborts the transaction for a deadlock or a
/// serialization failure, whether inside `f` or at commit, the whole unit of work runs once more in a new
/// transaction. A second abort is returned as it is: the unexpected tier.
pub async fn run_retrying<T>(
    db: &Db,
    scope: Scope,
    f: impl AsyncFn(&mut DbTx) -> Result<T, TreeError>,
) -> Result<T, TreeError> {
    match run_once(db, scope, &f).await {
        Err(error) if aborted(&error) => run_once(db, scope, &f).await,
        outcome => outcome,
    }
}

async fn run_once<T>(
    db: &Db,
    scope: Scope,
    f: &impl AsyncFn(&mut DbTx) -> Result<T, TreeError>,
) -> Result<T, TreeError> {
    db.run(scope, async |tx| f(tx).await)
        .await
        .map_err(|error| match error {
            RunError::Closure(error) => error,
            RunError::Db(error) => TreeError::Database(error),
        })
}

fn aborted(error: &TreeError) -> bool {
    match error {
        TreeError::Database(DbError::Serialization) => true,
        TreeError::Database(DbError::Other(text)) => text.starts_with(DEADLOCK_PREFIX),
        _ => false,
    }
}
