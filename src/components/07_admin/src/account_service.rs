use std::sync::Arc;

use contracts::{
    AccountAdmin, AccountService, AccountServiceError, AccountView, Principal, QuotaAdmin,
    QuotaView, TreeReader, async_trait,
};

/// `AccountService` of 07 (`view`, `quota_view`). Every read is keyed by the principal's own account id, so no
/// call can return another account's values.
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

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> AccountServiceError {
    AccountServiceError::Unexpected(Box::new(error))
}

#[async_trait]
impl AccountService for AccountServiceImpl {
    /// The role is the principal's, as the authenticator resolved it; 01 supplies the name and identities, 07a
    /// the usage and 02 the root folder.
    async fn view(&self, principal: &Principal) -> Result<AccountView, AccountServiceError> {
        let account = principal.account_id;
        let profile = self.accounts.profile(account).await.map_err(unexpected)?;
        let quota = self.quota.view(account).await.map_err(unexpected)?;
        let root = self.tree.root_node(account).await.map_err(unexpected)?;
        Ok(AccountView {
            id: account,
            display_name: profile.display_name,
            role: principal.role,
            identities: profile.identities,
            quota,
            root_node_id: root.id,
        })
    }

    async fn quota_view(&self, principal: &Principal) -> Result<QuotaView, AccountServiceError> {
        self.quota
            .view(principal.account_id)
            .await
            .map_err(unexpected)
    }
}
