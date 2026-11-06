use std::sync::Arc;

use contracts::{AccountAdmin, QuotaAdmin, TreeReader};

/// `AccountService` of 07 (`view`, `quota_view`).
pub struct AccountServiceImpl {
    accounts: Arc<dyn AccountAdmin>,
    quota: Arc<dyn QuotaAdmin>,
    tree: Arc<dyn TreeReader>,
}

impl AccountServiceImpl {
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        quota: Arc<dyn QuotaAdmin>,
        tree: Arc<dyn TreeReader>,
    ) -> Self {
        AccountServiceImpl {
            accounts,
            quota,
            tree,
        }
    }

    pub fn accounts(&self) -> &Arc<dyn AccountAdmin> {
        &self.accounts
    }

    pub fn quota(&self) -> &Arc<dyn QuotaAdmin> {
        &self.quota
    }

    pub fn tree(&self) -> &Arc<dyn TreeReader> {
        &self.tree
    }
}
