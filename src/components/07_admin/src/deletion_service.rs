use std::sync::Arc;

use contracts::AccountAdmin;

use crate::AdminConfig;

/// `AccountDeletionService` of 07 (`request`).
pub struct AccountDeletionServiceImpl {
    accounts: Arc<dyn AccountAdmin>,
    config: AdminConfig,
}

impl AccountDeletionServiceImpl {
    pub fn new(accounts: Arc<dyn AccountAdmin>, config: AdminConfig) -> Self {
        AccountDeletionServiceImpl { accounts, config }
    }

    pub fn accounts(&self) -> &Arc<dyn AccountAdmin> {
        &self.accounts
    }

    pub fn config(&self) -> &AdminConfig {
        &self.config
    }
}
