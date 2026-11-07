//! A double of 01's `AccountAdmin` over the real `accounts`, `sessions` and `device_tokens` tables, because
//! 07 may not link the identity crate (ADR 015, `crate_graph`). It keeps the two properties 07 relies on:
//! `set_role` and `set_state` run on the caller's transaction, and disabling revokes the credentials in it.
//! Listing is oldest first, as 01 does it. The calls the user administration does not make answer with a typed
//! failure.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use contracts::{
    AccountAdmin, AccountAdminError, AccountFilter, AccountInfo, AccountState, AccountStatus, Page,
    Role, async_trait,
};
use sc_clock::Timestamp;
use sc_db::{Db, DbTx, Scope};
use sc_testdb::START;
use uuid::Uuid;

const NOT_CALLED: &str = "the user administration does not call this port";

type ListedRow = (Uuid, String, String, String, Option<i64>);

pub struct AccountsDouble {
    reader: Db,
    pub list_calls: AtomicUsize,
    pub max_list_limit: AtomicU32,
    pub mutations: AtomicUsize,
    pub fail_set_state: AtomicBool,
}

impl AccountsDouble {
    /// `reader` is a handle of the schema owner, which row-level security does not hide rows from.
    pub fn new(reader: Db) -> Self {
        AccountsDouble {
            reader,
            list_calls: AtomicUsize::new(0),
            max_list_limit: AtomicU32::new(0),
            mutations: AtomicUsize::new(0),
            fail_set_state: AtomicBool::new(false),
        }
    }
}

fn failure(text: &str) -> AccountAdminError {
    AccountAdminError::Unexpected(text.into())
}

fn database(error: impl std::error::Error + Send + Sync + 'static) -> AccountAdminError {
    AccountAdminError::Unexpected(Box::new(error))
}

#[async_trait]
impl AccountAdmin for AccountsDouble {
    async fn list_accounts(
        &self,
        _filter: AccountFilter,
        page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.max_list_limit.fetch_max(page.limit, Ordering::SeqCst);
        let rows = self
            .reader
            .run(Scope::Account(Uuid::nil()), async |tx| {
                sqlx::query_as::<_, ListedRow>(
                    "SELECT a.id, a.display_name, a.role, a.state, \
                     (SELECT (extract(epoch FROM max(i.last_login_at)) * 1000000)::bigint \
                      FROM identities i WHERE i.account_id = a.id) \
                     FROM accounts a ORDER BY a.created_at, a.id LIMIT $1 OFFSET $2",
                )
                .bind(i64::from(page.limit))
                .bind(i64::try_from(page.offset).unwrap_or(i64::MAX))
                .fetch_all(tx.conn())
                .await
            })
            .await
            .map_err(database)?;
        rows.into_iter()
            .map(|(id, display_name, role, state, last_login)| {
                Ok(AccountInfo {
                    id,
                    display_name,
                    role: match role.as_str() {
                        "admin" => Role::Admin,
                        "user" => Role::User,
                        other => return Err(failure(other)),
                    },
                    state: match state.as_str() {
                        "active" => AccountStatus::Active,
                        "disabled" => AccountStatus::Disabled,
                        "deleting" => AccountStatus::Deleting,
                        other => return Err(failure(other)),
                    },
                    last_login: last_login.map(Timestamp::from_unix_micros),
                })
            })
            .collect()
    }

    async fn lock_active_admins(&self, _tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError> {
        Err(failure(NOT_CALLED))
    }

    async fn set_role(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        role: Role,
    ) -> Result<(), AccountAdminError> {
        self.mutations.fetch_add(1, Ordering::SeqCst);
        let text = match role {
            Role::User => "user",
            Role::Admin => "admin",
        };
        sqlx::query("UPDATE accounts SET role = $2 WHERE id = $1")
            .bind(account)
            .bind(text)
            .execute(tx.conn())
            .await
            .map(drop)
            .map_err(database)
    }

    async fn set_state(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        state: AccountState,
    ) -> Result<(), AccountAdminError> {
        self.mutations.fetch_add(1, Ordering::SeqCst);
        if self.fail_set_state.load(Ordering::SeqCst) {
            return Err(failure("injected set_state failure"));
        }
        let text = match state {
            AccountState::Active => "active",
            AccountState::Disabled => "disabled",
        };
        sqlx::query("UPDATE accounts SET state = $2 WHERE id = $1")
            .bind(account)
            .bind(text)
            .execute(tx.conn())
            .await
            .map_err(database)?;
        if state == AccountState::Disabled {
            for table in [
                "UPDATE sessions SET revoked_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', \
                 revoke_reason = 'admin' WHERE account_id = $1 AND revoked_at IS NULL",
                "UPDATE device_tokens SET revoked_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', \
                 revoke_reason = 'admin' WHERE account_id = $1 AND revoked_at IS NULL",
            ] {
                sqlx::query(table)
                    .bind(account)
                    .bind(START.as_unix_micros())
                    .execute(tx.conn())
                    .await
                    .map_err(database)?;
            }
        }
        Ok(())
    }

    async fn begin_deletion(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
    ) -> Result<(), AccountAdminError> {
        Err(failure(NOT_CALLED))
    }

    async fn finalize_deletion(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
    ) -> Result<(), AccountAdminError> {
        Err(failure(NOT_CALLED))
    }
}
