//! Ports of 07 admin. Every mutating call writes its `admin_action` audit row inside the provider.

use async_trait::async_trait;
use sc_clock::Timestamp;
use thiserror::Error;
use uuid::Uuid;

use crate::common::{Fault, Page};
use crate::identity::{AccountState, AccountStatus, Principal, Role};
use crate::quota::QuotaView;
use crate::registries::HealthItem;

/// Who performs an admin call: a signed-in admin or the operator CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Admin(Principal),
    Operator,
}

/// `limit` is the per-account limit; `None` means the installation default applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserView {
    pub account: Uuid,
    pub role: Role,
    pub state: AccountStatus,
    pub last_login: Option<Timestamp>,
    pub used: i64,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub items: Vec<(String, HealthItem)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityView {
    pub provider: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub id: Uuid,
    pub display_name: String,
    pub role: Role,
    pub identities: Vec<IdentityView>,
    pub quota: QuotaView,
    pub root_node_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletionAccepted {
    pub deadline: Timestamp,
}

#[derive(Debug, Error)]
pub enum AdminError {
    #[error("last admin")]
    LastAdmin,
    #[error("not found")]
    NotFound,
    #[error("invalid limit")]
    InvalidLimit,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait AdminService: Send + Sync {
    async fn list_users(&self, actor: &Actor, page: Page)
    -> Result<Vec<AdminUserView>, AdminError>;

    /// Applies `role` and `state` when present, in one transaction under the last-admin rule.
    async fn update_user(
        &self,
        actor: &Actor,
        account: Uuid,
        role: Option<Role>,
        state: Option<AccountState>,
    ) -> Result<(), AdminError>;

    async fn set_quota(
        &self,
        actor: &Actor,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), AdminError>;

    /// A contributor that fails or times out is reported as an item, so this cannot fail.
    async fn health(&self, actor: &Actor) -> HealthReport;
}

#[derive(Debug, Error)]
pub enum AccountServiceError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait AccountService: Send + Sync {
    async fn view(&self, principal: &Principal) -> Result<AccountView, AccountServiceError>;

    async fn quota_view(&self, principal: &Principal) -> Result<QuotaView, AccountServiceError>;
}

#[derive(Debug, Error)]
pub enum DeletionError {
    /// A device token cannot delete the account.
    #[error("forbidden")]
    Forbidden,
    #[error("last admin")]
    LastAdmin,
    #[error("confirmation mismatch")]
    ConfirmationMismatch,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait AccountDeletionService: Send + Sync {
    async fn request(
        &self,
        principal: &Principal,
        confirmation: &str,
    ) -> Result<DeletionAccepted, DeletionError>;
}

#[derive(Debug, Error)]
pub enum DevSeedError {
    /// The variant name is open in 07; this is the refusal when `environment = production`.
    #[error("refused in production")]
    RefusedInProduction,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait DevSeed: Send + Sync {
    async fn seed(&self) -> Result<(), DevSeedError>;
}
