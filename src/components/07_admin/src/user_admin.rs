use contracts::{
    AccountFilter, AccountState, Actor, AdminError, AdminUserView, Page, QuotaAdminError, Role,
};
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, SubjectRef};
use sc_db::{RunError, Scope};
use uuid::Uuid;

use crate::actor::authorize;
use crate::admin_service::AdminServiceImpl;
use crate::error::AdminCallError;
use crate::listing::{merge, page_limit};

const SUBJECT_TYPE: &str = "account";

impl AdminServiceImpl {
    /// One page of accounts with their usage: one `list_accounts` call and one `views` call.
    pub async fn list_users(
        &self,
        actor: &Actor,
        page: Page,
    ) -> Result<Vec<AdminUserView>, AdminCallError> {
        authorize(actor)?;
        let limit = page_limit(page.limit, self.config.admin_list_page_size);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let accounts = self
            .accounts
            .list_accounts(
                AccountFilter::default(),
                Page {
                    offset: page.offset,
                    limit,
                },
            )
            .await
            .map_err(unexpected)?;
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = accounts.iter().map(|account| account.id).collect();
        let usage = self.quota.views(&ids).await.map_err(unexpected)?;
        Ok(merge(accounts, &usage))
    }

    /// Applies `role` and `state` in one transaction in the account's scope, each followed by its audit row.
    /// Disabling revokes the account's credentials inside `AccountAdmin::set_state`, so the revocation rolls
    /// back with the audit row.
    pub async fn update_user(
        &self,
        actor: &Actor,
        account: Uuid,
        role: Option<Role>,
        state: Option<AccountState>,
    ) -> Result<(), AdminCallError> {
        authorize(actor)?;
        self.require_known(account).await?;
        if role.is_none() && state.is_none() {
            return Ok(());
        }
        let result = self
            .db
            .run(Scope::Account(account), async |tx| {
                if let Some(role) = role {
                    self.accounts
                        .set_role(tx, account, role)
                        .await
                        .map_err(unexpected)?;
                }
                if let Some(state) = state {
                    self.accounts
                        .set_state(tx, account, state)
                        .await
                        .map_err(unexpected)?;
                }
                if let Some(role) = role {
                    let event = admin_action(actor, account, role_action(role));
                    self.audit.record_in(tx, event).await.map_err(unexpected)?;
                }
                if let Some(state) = state {
                    let event = admin_action(actor, account, state_action(state));
                    self.audit.record_in(tx, event).await.map_err(unexpected)?;
                }
                Ok(())
            })
            .await;
        result.map_err(|error| match error {
            RunError::Closure(error) => error,
            RunError::Db(error) => unexpected(error),
        })?;
        Ok(())
    }

    /// `None` restores the installation default. 07a commits the limit in its own transaction, so the audit
    /// row follows it in another; a failed audit write is reported after the limit has changed.
    pub async fn set_quota(
        &self,
        actor: &Actor,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), AdminCallError> {
        authorize(actor)?;
        self.quota
            .set_limit(account, limit)
            .await
            .map_err(|error| match error {
                QuotaAdminError::NotFound => AdminError::NotFound,
                QuotaAdminError::InvalidLimit => AdminError::InvalidLimit,
                QuotaAdminError::Unexpected(fault) => AdminError::Unexpected(fault),
            })?;
        let action = if limit.is_some() {
            "quota_limit_set"
        } else {
            "quota_limit_default"
        };
        self.audit
            .record(admin_action(actor, account, action))
            .await
            .map_err(unexpected)?;
        Ok(())
    }

    /// 01 reports an unknown account to `set_role` and `set_state` only as an untyped failure; 07a's `view` is
    /// the typed answer, since every account has a ledger row from creation until its erase.
    async fn require_known(&self, account: Uuid) -> Result<(), AdminError> {
        match self.quota.view(account).await {
            Ok(_) => Ok(()),
            Err(QuotaAdminError::NotFound) => Err(AdminError::NotFound),
            Err(error) => Err(unexpected(error)),
        }
    }
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> AdminError {
    AdminError::Unexpected(Box::new(error))
}

fn role_action(role: Role) -> &'static str {
    match role {
        Role::User => "role_user",
        Role::Admin => "role_admin",
    }
}

fn state_action(state: AccountState) -> &'static str {
    match state {
        AccountState::Active => "state_active",
        AccountState::Disabled => "state_disabled",
    }
}

/// Ids and one action code; no name, email or address can enter an `AuditEvent`.
fn admin_action(actor: &Actor, subject: Uuid, action: &'static str) -> AuditEvent {
    let (actor_account_id, actor_session_id) = match actor {
        Actor::Admin(principal) => (
            Some(principal.account_id),
            Some(principal.session_or_device_id),
        ),
        Actor::Operator => (None, None),
    };
    AuditEvent {
        actor_account_id,
        actor_session_id,
        subject: Some(SubjectRef {
            subject_type: SUBJECT_TYPE,
            id: subject,
        }),
        detail: AuditDetail::new().with_code("action", action),
        ..AuditEvent::new(AuditEventKind::AdminAction)
    }
}
