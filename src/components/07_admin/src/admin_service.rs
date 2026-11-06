use std::sync::Arc;

use contracts::{AccountAdmin, HealthContributor, QuotaAdmin};

use crate::AdminConfig;

/// `AdminService` of 07 (`list_users`, `update_user`, `set_quota`, `health`). The contributor list keeps the
/// order it is given.
pub struct AdminServiceImpl {
    accounts: Arc<dyn AccountAdmin>,
    quota: Arc<dyn QuotaAdmin>,
    health: Vec<Arc<dyn HealthContributor>>,
    config: AdminConfig,
}

impl AdminServiceImpl {
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        quota: Arc<dyn QuotaAdmin>,
        health: Vec<Arc<dyn HealthContributor>>,
        config: AdminConfig,
    ) -> Self {
        AdminServiceImpl {
            accounts,
            quota,
            health,
            config,
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
