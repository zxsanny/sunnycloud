use std::sync::Arc;

use contracts::{AccountAdmin, HealthContributor, QuotaAdmin};
use sc_audit::AuditWriter;
use sc_db::Db;

use crate::AdminConfig;

/// `AdminService` of 07 (`list_users`, `update_user`, `set_quota`, `health`). The contributor list keeps the
/// order it is given.
pub struct AdminServiceImpl {
    pub(crate) accounts: Arc<dyn AccountAdmin>,
    pub(crate) quota: Arc<dyn QuotaAdmin>,
    health: Vec<Arc<dyn HealthContributor>>,
    pub(crate) config: AdminConfig,
    pub(crate) audit: AuditWriter,
    pub(crate) db: Db,
}

impl AdminServiceImpl {
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        quota: Arc<dyn QuotaAdmin>,
        health: Vec<Arc<dyn HealthContributor>>,
        config: AdminConfig,
        audit: AuditWriter,
        db: Db,
    ) -> Self {
        AdminServiceImpl {
            accounts,
            quota,
            health,
            config,
            audit,
            db,
        }
    }

    pub fn accounts(&self) -> &Arc<dyn AccountAdmin> {
        &self.accounts
    }

    pub fn quota(&self) -> &Arc<dyn QuotaAdmin> {
        &self.quota
    }

    pub fn health_contributors(&self) -> &[Arc<dyn HealthContributor>] {
        &self.health
    }

    pub fn config(&self) -> &AdminConfig {
        &self.config
    }
}
