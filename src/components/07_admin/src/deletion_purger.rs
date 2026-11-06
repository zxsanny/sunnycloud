use std::sync::Arc;

use contracts::{AccountAdmin, AccountLifecycleHook};

use crate::AdminConfig;

/// The deletion purger (a `Janitor` once its behaviour lands). The hook list is the call order: it is kept as
/// given and never sorted.
pub struct DeletionPurger {
    accounts: Arc<dyn AccountAdmin>,
    hooks: Vec<Arc<dyn AccountLifecycleHook>>,
    config: AdminConfig,
}

impl DeletionPurger {
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        hooks: Vec<Arc<dyn AccountLifecycleHook>>,
        config: AdminConfig,
    ) -> Self {
        DeletionPurger {
            accounts,
            hooks,
            config,
        }
    }

    pub fn accounts(&self) -> &Arc<dyn AccountAdmin> {
        &self.accounts
    }

    pub fn hooks(&self) -> &[Arc<dyn AccountLifecycleHook>] {
        &self.hooks
    }

    pub fn config(&self) -> &AdminConfig {
        &self.config
    }
}
