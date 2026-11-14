use async_trait::async_trait;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use super::model::{AccountFilter, AccountInfo, AccountProfile, AccountState, Role};
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

    /// The account's display name and linked identities, oldest identity first; read-only, own short
    /// transaction in the account's scope. An unknown account is `Unexpected`.
    async fn profile(&self, account: Uuid) -> Result<AccountProfile, AccountAdminError>;

    /// Locks the active admin rows (role admin, state active; ordered by id) for the last-admin check, before
    /// any target row. A lost lock race is retried once inside the call; a second one is `Unexpected`.
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

    /// The purger's per-step claim: takes the `deleting` account row with `FOR NO KEY UPDATE SKIP LOCKED` until
    /// the caller's transaction ends. `false` when the row is held by another transaction or the account is not
    /// `deleting` (or gone); the caller then skips the step. The lock mode leaves `FOR KEY SHARE` free, so an
    /// erase hook on its own connection can still insert a row that references the account.
    async fn lock_deleting(&self, tx: &mut DbTx, account: Uuid) -> Result<bool, AccountAdminError>;

    /// Called by 07 only after every lifecycle hook reports nothing remaining.
    async fn finalize_deletion(
        &self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(), AccountAdminError>;
}
