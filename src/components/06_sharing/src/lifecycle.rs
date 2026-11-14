//! Sharing's part of the account lifecycle (`AccountLifecycleHook`, AZ-3702). A new account has no links; the erase
//! removes the account's links in bounded batches when 07's purger asks. A link whose row is gone answers as
//! missing, and while its target is a tombstone it already does.

use contracts::{AccountLifecycleHook, EraseProgress, LifecycleError, async_trait};
use sc_db::{Db, DbTx, RunError, Scope};
use uuid::Uuid;

use crate::share_repo;

/// Registered by 12 in the hook list; 07 runs it first.
#[derive(Debug, Clone)]
pub struct PgShareLifecycle {
    db: Db,
}

impl PgShareLifecycle {
    pub fn new(db: Db) -> Self {
        PgShareLifecycle { db }
    }
}

#[async_trait]
impl AccountLifecycleHook for PgShareLifecycle {
    /// A new account has no links: no statement, no I/O.
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// One short account-scoped transaction per call: deletes up to `limit` links, then counts what is left. An
    /// account with no links, erased or unknown, reports 0 again.
    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        let remaining = self
            .db
            .run(Scope::Account(account), async |tx| {
                share_repo::delete_batch(tx, account, limit).await?;
                share_repo::count(tx, account).await
            })
            .await
            .map_err(|error| match error {
                RunError::Closure(error) | RunError::Db(error) => {
                    LifecycleError::Unexpected(Box::new(error))
                }
            })?;
        let remaining = u64::try_from(remaining)
            .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        Ok(EraseProgress { remaining })
    }
}
