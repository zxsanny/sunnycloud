use std::sync::Arc;

use contracts::{
    AccountAdmin, AccountState, Actor, AdminError, AdminService, AdminUserView, HealthContributor,
    HealthReport, Page, QuotaAdmin, Role, async_trait,
};
use sc_audit::AuditWriter;
use sc_db::Db;
use uuid::Uuid;

use crate::AdminConfig;
use crate::actor::authorize;

/// `AdminService` of 07 (`list_users`, `update_user`, `set_quota`, `health`). The contributor list keeps the
/// order it is given.
pub struct AdminServiceImpl {
    pub(crate) accounts: Arc<dyn AccountAdmin>,
    pub(crate) quota: Arc<dyn QuotaAdmin>,
    pub(crate) health: Vec<Arc<dyn HealthContributor>>,
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

/// Every operation first authorizes the actor: a caller who may not administer gets `Forbidden` before anything
/// is read or written. The bodies are in `crate::user_admin` and `crate::health`.
#[async_trait]
impl AdminService for AdminServiceImpl {
    async fn list_users(
        &self,
        actor: &Actor,
        page: Page,
    ) -> Result<Vec<AdminUserView>, AdminError> {
        authorize(actor)?;
        self.users_page(page).await
    }

    async fn update_user(
        &self,
        actor: &Actor,
        account: Uuid,
        role: Option<Role>,
        state: Option<AccountState>,
    ) -> Result<(), AdminError> {
        authorize(actor)?;
        self.apply_update(actor, account, role, state).await
    }

    async fn set_quota(
        &self,
        actor: &Actor,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), AdminError> {
        authorize(actor)?;
        self.apply_quota(actor, account, limit).await
    }

    async fn health(&self, actor: &Actor) -> Result<HealthReport, AdminError> {
        authorize(actor)?;
        Ok(self.run_checks().await)
    }
}
