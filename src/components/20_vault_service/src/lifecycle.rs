use contracts::{AccountLifecycleHook, EraseProgress, LifecycleError, async_trait};
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use uuid::Uuid;

use crate::vault_repo;

/// The vault's part of the account lifecycle. A vault is opt-in, so account creation adds nothing; erase
/// removes the account's `vaults` rows.
///
/// 12 registers this hook before 02's (decision 2026-11-05). Since `*_cascade_vault_on_root_purge` (owner,
/// 2026-11-13) 02's purge of the root folder would also remove the vault row, but the erase does not rely on it.
#[derive(Debug, Clone)]
pub struct PgVaultLifecycle {
    db: Db,
}

impl PgVaultLifecycle {
    pub fn new(db: Db) -> Self {
        PgVaultLifecycle { db }
    }
}

#[async_trait]
impl AccountLifecycleHook for PgVaultLifecycle {
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Ok(())
    }

    /// One short transaction per call: deletes up to `limit` rows, then counts what is left.
    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        let remaining: i64 = self
            .db
            .run(Scope::Account(account), async |tx| {
                vault_repo::delete_batch(tx, account, limit).await?;
                vault_repo::count(tx, account).await
            })
            .await
            .map_err(|error| {
                let error = match error {
                    RunError::Closure(error) => DbError::from(error),
                    RunError::Db(error) => error,
                };
                LifecycleError::Unexpected(Box::new(error))
            })?;
        let remaining = u64::try_from(remaining)
            .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        Ok(EraseProgress { remaining })
    }
}
