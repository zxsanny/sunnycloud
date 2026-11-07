use std::collections::HashMap;

use contracts::{
    AccountLifecycleHook, EraseProgress, LifecycleError, QuotaAdminError, QuotaView, async_trait,
};
use sc_db::{Db, DbTx, Scope};
use sc_observability::log_info;
use uuid::Uuid;

use crate::admin_repo;
use crate::config::{MAX_LIMIT_BYTES, QuotaConfig};
use crate::error::{admin_error, db_error, lifecycle_error};

/// The admin calls and the lifecycle hook over the `quota` table. The admin calls open their own short
/// transaction; `initialize` runs in the caller's. `recompute`, the last method of `QuotaAdmin`, lands with
/// E-11/c04, which also implements the trait for this type.
#[derive(Debug, Clone)]
pub struct PgQuotaAdmin {
    db: Db,
    default_quota_bytes: i64,
}

impl PgQuotaAdmin {
    pub fn new(db: Db, config: QuotaConfig) -> Self {
        PgQuotaAdmin {
            db,
            default_quota_bytes: config.default_quota_bytes,
        }
    }

    pub async fn view(&self, account: Uuid) -> Result<QuotaView, QuotaAdminError> {
        self.db
            .run(Scope::Account(account), async |tx| {
                admin_repo::view(tx, account, self.default_quota_bytes).await
            })
            .await
            .map_err(|error| admin_error(db_error(error)))?
            .ok_or(QuotaAdminError::NotFound)
    }

    /// Unknown ids are omitted.
    pub async fn views(
        &self,
        accounts: &[Uuid],
    ) -> Result<HashMap<Uuid, QuotaView>, QuotaAdminError> {
        admin_repo::views(&self.db, accounts, self.default_quota_bytes)
            .await
            .map_err(|error| admin_error(db_error(error)))
    }

    /// `None` restores the installation default. A limit below the current use is accepted.
    pub async fn set_limit(
        &self,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), QuotaAdminError> {
        if limit.is_some_and(|bytes| !(0..=MAX_LIMIT_BYTES).contains(&bytes)) {
            return Err(QuotaAdminError::InvalidLimit);
        }
        let found = self
            .db
            .run(Scope::Account(account), async |tx| {
                admin_repo::set_limit(tx, account, limit).await
            })
            .await
            .map_err(|error| admin_error(db_error(error)))?;
        if !found {
            return Err(QuotaAdminError::NotFound);
        }
        log_info!("quota limit changed", account_id = account);
        Ok(())
    }
}

#[async_trait]
impl AccountLifecycleHook for PgQuotaAdmin {
    async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError> {
        admin_repo::insert(tx, account)
            .await
            .map_err(lifecycle_error)
    }

    async fn erase_batch(
        &self,
        account: Uuid,
        _limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        self.db
            .run(Scope::Account(account), async |tx| {
                admin_repo::delete(tx, account).await
            })
            .await
            .map_err(|error| lifecycle_error(db_error(error)))?;
        Ok(EraseProgress { remaining: 0 })
    }
}
