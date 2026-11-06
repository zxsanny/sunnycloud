use std::sync::Arc;

use contracts::{
    AccountAdmin, AccountAdminError, AccountFilter, AccountGuard, AccountGuardError, AccountInfo,
    AccountState, AccountStatus, Page, RevokeReason, Role, async_trait,
};
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, DbTx};
use uuid::Uuid;

use crate::account_repo::{self, ListedAccount};
use crate::auth_repo;
use crate::device_grant_repo;
use crate::error::IdentityError;
use crate::session_repo;

const ACTIVE: &str = "active";
const DISABLED: &str = "disabled";
const DELETING: &str = "deleting";

/// The account operations 07 combines with its own policy, and the guard 02 calls first in a commit.
///
/// Every method that takes a transaction runs on it and never opens, commits or rolls back one, so the
/// caller's rollback undoes all of it. The caller's scope must cover the account it names (its own account's
/// scope, or system scope): row-level security hides the account's sessions, tokens and grants from any
/// other scope.
pub struct IdentityAccounts {
    db: Db,
    clock: Arc<dyn Clock>,
}

impl IdentityAccounts {
    pub fn new(db: Db, clock: Arc<dyn Clock>) -> Self {
        IdentityAccounts { db, clock }
    }

    /// Revokes the account's sessions and device tokens with `reason`.
    async fn revoke_credentials(
        tx: &mut DbTx,
        account: Uuid,
        at: Timestamp,
        reason: RevokeReason,
    ) -> Result<(), AccountAdminError> {
        let reason = match reason {
            RevokeReason::Admin => "admin",
            RevokeReason::AccountDeleted => "account_deleted",
            RevokeReason::User => "user",
            RevokeReason::RefreshReuse => "refresh_reuse",
        };
        session_repo::revoke_all(tx, account, at, reason)
            .await
            .map_err(unexpected)?;
        device_grant_repo::revoke_all_devices(tx, account, at, reason)
            .await
            .map_err(unexpected)
    }

    /// The state of the account under its row lock; an unknown account is an error.
    async fn locked_state(tx: &mut DbTx, account: Uuid) -> Result<String, AccountAdminError> {
        account_repo::lock(tx, account)
            .await
            .map_err(unexpected)?
            .ok_or_else(|| unexpected(IdentityError::AccountNotFound))
    }
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> AccountAdminError {
    AccountAdminError::Unexpected(Box::new(error))
}

/// The guard's decision: only an existing, active account passes.
fn admits(state: Option<&str>) -> bool {
    state == Some(ACTIVE)
}

fn role_text(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Admin => "admin",
    }
}

fn status_text(status: AccountStatus) -> &'static str {
    match status {
        AccountStatus::Active => ACTIVE,
        AccountStatus::Disabled => DISABLED,
        AccountStatus::Deleting => DELETING,
    }
}

fn account_info(row: ListedAccount) -> Result<AccountInfo, IdentityError> {
    let role = match row.role.as_str() {
        "user" => Role::User,
        "admin" => Role::Admin,
        _ => {
            return Err(IdentityError::UnexpectedColumnValue {
                column: "accounts.role",
                value: row.role,
            });
        }
    };
    let state = match row.state.as_str() {
        ACTIVE => AccountStatus::Active,
        DISABLED => AccountStatus::Disabled,
        DELETING => AccountStatus::Deleting,
        _ => {
            return Err(IdentityError::UnexpectedColumnValue {
                column: "accounts.state",
                value: row.state,
            });
        }
    };
    Ok(AccountInfo {
        id: row.id,
        display_name: row.display_name,
        role,
        state,
        last_login: row.last_login_micros.map(Timestamp::from_unix_micros),
    })
}

#[async_trait]
impl AccountAdmin for IdentityAccounts {
    /// Oldest account first. Runs in its own short transaction in system scope: the last login is read from
    /// the identities of every account.
    async fn list_accounts(
        &self,
        filter: AccountFilter,
        page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError> {
        let rows = auth_repo::list_accounts(
            &self.db,
            filter.role.map(role_text),
            filter.state.map(status_text),
            i64::from(page.limit),
            i64::try_from(page.offset).unwrap_or(i64::MAX),
        )
        .await
        .map_err(unexpected)?;
        rows.into_iter()
            .map(|row| account_info(row).map_err(unexpected))
            .collect()
    }

    async fn lock_active_admins(&self, tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError> {
        account_repo::lock_active_admins(tx)
            .await
            .map_err(unexpected)
    }

    async fn set_role(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        role: Role,
    ) -> Result<(), AccountAdminError> {
        if account_repo::set_role(tx, account, role_text(role))
            .await
            .map_err(unexpected)?
        {
            Ok(())
        } else {
            Err(unexpected(IdentityError::AccountNotFound))
        }
    }

    /// A `deleting` account stays `deleting`: the change is refused.
    async fn set_state(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        state: AccountState,
    ) -> Result<(), AccountAdminError> {
        let current = Self::locked_state(tx, account).await?;
        if current == DELETING {
            return Err(unexpected(IdentityError::AccountStateRefused {
                operation: "set_state",
                state: current,
            }));
        }
        match state {
            AccountState::Active => account_repo::set_state(tx, account, ACTIVE)
                .await
                .map_err(unexpected),
            AccountState::Disabled => {
                account_repo::set_state(tx, account, DISABLED)
                    .await
                    .map_err(unexpected)?;
                Self::revoke_credentials(tx, account, self.clock.now_utc(), RevokeReason::Admin)
                    .await
            }
        }
    }

    /// Repeating the call keeps the first `deletion_requested_at` and revokes whatever is still live.
    async fn begin_deletion(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountAdminError> {
        Self::locked_state(tx, account).await?;
        let now = self.clock.now_utc();
        account_repo::mark_deleting(tx, account, now)
            .await
            .map_err(unexpected)?;
        Self::revoke_credentials(tx, account, now, RevokeReason::AccountDeleted).await?;
        device_grant_repo::deny_open_grants(tx, account)
            .await
            .map_err(unexpected)
    }

    /// An account that is already gone is finished; an account that is not `deleting` is refused, so a wrong
    /// call cannot erase a live account.
    async fn finalize_deletion(
        &self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(), AccountAdminError> {
        match account_repo::lock(tx, account).await.map_err(unexpected)? {
            None => Ok(()),
            Some(state) if state == DELETING => account_repo::delete_with_owned_rows(tx, account)
                .await
                .map_err(unexpected),
            Some(state) => Err(unexpected(IdentityError::AccountStateRefused {
                operation: "finalize_deletion",
                state,
            })),
        }
    }
}

#[async_trait]
impl AccountGuard for IdentityAccounts {
    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountGuardError> {
        let state = account_repo::state_shared(tx, account)
            .await
            .map_err(|error| AccountGuardError::Unexpected(Box::new(error)))?;
        if admits(state.as_deref()) {
            Ok(())
        } else {
            Err(AccountGuardError::AccountNotActive)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_existing_active_account_passes_the_guard() {
        // Arrange
        let states = [
            Some(ACTIVE),
            Some(DISABLED),
            Some(DELETING),
            Some("unknown"),
            None,
        ];

        // Act
        let decisions = states.map(admits);

        // Assert
        assert_eq!(decisions, [true, false, false, false, false]);
    }
}
