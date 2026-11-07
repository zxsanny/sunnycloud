use std::collections::HashMap;

use contracts::{
    AccountLifecycleHook, Drift, EraseProgress, LifecycleError, QuotaAdmin, QuotaAdminError,
    QuotaView, async_trait,
};
use sc_db::{Db, DbTx, Scope};
use sc_observability::{log_info, log_warn};
use uuid::Uuid;

use crate::admin_repo;
use crate::config::{MAX_LIMIT_BYTES, QuotaConfig};
use crate::error::{admin_error, db_error, lifecycle_error};

/// Drift of both counters of one account: `stored` is the value in the row, `computed` the one the caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterDrift {
    pub used: Drift,
    pub reserved: Drift,
}

impl CounterDrift {
    pub fn found(&self) -> bool {
        self.used.stored != self.used.computed || self.reserved.stored != self.reserved.computed
    }
}

/// The admin calls and the lifecycle hook over the `quota` table. The admin calls open their own short
/// transaction; `initialize` runs in the caller's.
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

    /// Both counters against the supplied truth, for the doctor; `QuotaAdmin::recompute` returns only the
    /// `used_bytes` column of this. One WARN with the account id per call that finds drift, whether or not it is
    /// repaired. A negative supplied value cannot be stored: with `repair` it fails as an unexpected database
    /// failure, without `repair` it is reported as drift.
    pub async fn recompute_columns(
        &self,
        account: Uuid,
        live_bytes: i64,
        reserved_bytes: i64,
        repair: bool,
    ) -> Result<CounterDrift, QuotaAdminError> {
        let (used, reserved) =
            admin_repo::recompute(&self.db, account, live_bytes, reserved_bytes, repair)
                .await
                .map_err(|error| admin_error(db_error(error)))?
                .ok_or(QuotaAdminError::NotFound)?;
        let drift = CounterDrift {
            used: Drift {
                stored: used,
                computed: live_bytes,
            },
            reserved: Drift {
                stored: reserved,
                computed: reserved_bytes,
            },
        };
        if drift.found() {
            log_warn!("quota drift found", account_id = account);
        }
        Ok(drift)
    }
}

#[async_trait]
impl QuotaAdmin for PgQuotaAdmin {
    async fn view(&self, account: Uuid) -> Result<QuotaView, QuotaAdminError> {
        PgQuotaAdmin::view(self, account).await
    }

    async fn views(&self, accounts: &[Uuid]) -> Result<HashMap<Uuid, QuotaView>, QuotaAdminError> {
        PgQuotaAdmin::views(self, accounts).await
    }

    async fn set_limit(&self, account: Uuid, limit: Option<i64>) -> Result<(), QuotaAdminError> {
        PgQuotaAdmin::set_limit(self, account, limit).await
    }

    /// The `used_bytes` column. `recompute_columns` reports both columns.
    async fn recompute(
        &self,
        account: Uuid,
        live_bytes: i64,
        reserved_bytes: i64,
        repair: bool,
    ) -> Result<Drift, QuotaAdminError> {
        Ok(self
            .recompute_columns(account, live_bytes, reserved_bytes, repair)
            .await?
            .used)
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
