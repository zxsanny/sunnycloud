//! A decorator around the real `AccountAdmin` that holds the transaction for a while after
//! `lock_active_admins` returns (ST-07-3): the delay between the guard's lock and its write.

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    AccountAdmin, AccountAdminError, AccountFilter, AccountInfo, AccountProfile, AccountState,
    Page, Role, async_trait,
};
use sc_db::DbTx;
use uuid::Uuid;

pub struct DelayedAccounts {
    inner: Arc<dyn AccountAdmin>,
    after_lock: Duration,
}

impl DelayedAccounts {
    pub fn wrap(inner: Arc<dyn AccountAdmin>, after_lock: Duration) -> Arc<dyn AccountAdmin> {
        Arc::new(DelayedAccounts { inner, after_lock })
    }
}

#[async_trait]
impl AccountAdmin for DelayedAccounts {
    async fn list_accounts(
        &self,
        filter: AccountFilter,
        page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError> {
        self.inner.list_accounts(filter, page).await
    }

    async fn profile(&self, account: Uuid) -> Result<AccountProfile, AccountAdminError> {
        self.inner.profile(account).await
    }

    async fn lock_active_admins(&self, tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError> {
        let locked = self.inner.lock_active_admins(tx).await;
        tokio::time::sleep(self.after_lock).await;
        locked
    }

    async fn set_role(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        role: Role,
    ) -> Result<(), AccountAdminError> {
        self.inner.set_role(tx, account, role).await
    }

    async fn set_state(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        state: AccountState,
    ) -> Result<(), AccountAdminError> {
        self.inner.set_state(tx, account, state).await
    }

    async fn begin_deletion(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountAdminError> {
        self.inner.begin_deletion(tx, account).await
    }

    async fn finalize_deletion(
        &self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(), AccountAdminError> {
        self.inner.finalize_deletion(tx, account).await
    }
}
