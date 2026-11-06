use async_trait::async_trait;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use super::model::{AccountFilter, AccountInfo, AccountState, Role};
use crate::common::{Fault, Page};

#[derive(Debug, Error)]
pub enum AccountAdminError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Account rows as 07 changes them. 07 decides policy; identity only applies the change in 07's transaction.
#[async_trait]
pub trait AccountAdmin: Send + Sync {
    async fn list_accounts(
        &self,
        filter: AccountFilter,
        page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError>;

    /// Locks the active admin rows for the last-admin check.
    async fn lock_active_admins(&self, tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError>;

    async fn set_role(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        role: Role,
    ) -> Result<(), AccountAdminError>;

    /// Disabling revokes the account's sessions and device tokens in the same transaction.
    async fn set_state(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        state: AccountState,
    ) -> Result<(), AccountAdminError>;

    async fn begin_deletion(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountAdminError>;

    /// Called by 07 only after every lifecycle hook reports nothing remaining.
    async fn finalize_deletion(
        &self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(), AccountAdminError>;
}
