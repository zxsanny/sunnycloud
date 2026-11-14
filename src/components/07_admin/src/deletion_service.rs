use std::sync::Arc;

use contracts::{
    AccountAdmin, AccountDeletionService, Actor, DeletionAccepted, DeletionError, Principal,
    PrincipalKind, async_trait,
};
use sc_audit::AuditWriter;
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, RunError, Scope};
use thiserror::Error;
use uuid::Uuid;

use crate::AdminConfig;
use crate::last_admin::{GuardRefusal, guard_removal};
use crate::user_admin::admin_action;

const DELETION_REQUESTED: &str = "deletion_requested";

/// `AccountDeletionService` of 07 (`request`, `request_as_operator`). The request only starts the deletion:
/// one short transaction with `AccountAdmin::begin_deletion` and its audit row; the purger erases later.
pub struct AccountDeletionServiceImpl {
    accounts: Arc<dyn AccountAdmin>,
    config: AdminConfig,
    audit: AuditWriter,
    db: Db,
    clock: Arc<dyn Clock>,
}

#[derive(Debug, Error)]
#[error("the deletion deadline does not fit a timestamp")]
struct DeadlineOverflow;

impl AccountDeletionServiceImpl {
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        config: AdminConfig,
        audit: AuditWriter,
        db: Db,
        clock: Arc<dyn Clock>,
    ) -> Self {
        AccountDeletionServiceImpl {
            accounts,
            config,
            audit,
            db,
            clock,
        }
    }

    pub fn accounts(&self) -> &Arc<dyn AccountAdmin> {
        &self.accounts
    }

    pub fn config(&self) -> &AdminConfig {
        &self.config
    }

    /// The last-admin guard, `begin_deletion` (01 marks the account `deleting` and revokes its sessions, device
    /// tokens and grants) and the audit row, in one transaction in the account's scope.
    async fn begin(&self, actor: &Actor, account: Uuid) -> Result<DeletionAccepted, DeletionError> {
        let deadline = deadline(self.clock.now_utc(), &self.config)?;
        self.db
            .run(Scope::Account(account), async |tx| {
                guard_removal(self.accounts.as_ref(), tx, account)
                    .await
                    .map_err(|refusal| match refusal {
                        GuardRefusal::LastAdmin => DeletionError::LastAdmin,
                        GuardRefusal::Port(error) => unexpected(error),
                    })?;
                self.accounts
                    .begin_deletion(tx, account)
                    .await
                    .map_err(unexpected)?;
                let event = admin_action(actor, account, DELETION_REQUESTED);
                self.audit.record_in(tx, event).await.map_err(unexpected)
            })
            .await
            .map_err(|error| match error {
                RunError::Closure(error) => error,
                RunError::Db(error) => unexpected(error),
            })?;
        Ok(DeletionAccepted { deadline })
    }
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> DeletionError {
    DeletionError::Unexpected(Box::new(error))
}

/// The confirmation is the account's own id as lowercase hyphenated UUID text, compared exactly: no trimming
/// and no case folding (decided 2026-11-07).
fn confirms(account: Uuid, confirmation: &str) -> bool {
    let mut buffer = Uuid::encode_buffer();
    confirmation == account.hyphenated().encode_lower(&mut buffer)
}

fn deadline(now: Timestamp, config: &AdminConfig) -> Result<Timestamp, DeletionError> {
    now.checked_add(config.account_delete_deadline)
        .ok_or_else(|| unexpected(DeadlineOverflow))
}

#[async_trait]
impl AccountDeletionService for AccountDeletionServiceImpl {
    /// A device token is refused before the confirmation is read; either refusal changes nothing.
    async fn request(
        &self,
        principal: &Principal,
        confirmation: &str,
    ) -> Result<DeletionAccepted, DeletionError> {
        if principal.kind != PrincipalKind::Session {
            return Err(DeletionError::Forbidden);
        }
        if !confirms(principal.account_id, confirmation) {
            return Err(DeletionError::ConfirmationMismatch);
        }
        self.begin(&Actor::Admin(principal.clone()), principal.account_id)
            .await
    }

    /// `--yes` is the confirmation. An unknown account is `Unexpected`: the contract has no `NotFound` here.
    async fn request_as_operator(&self, account: Uuid) -> Result<DeletionAccepted, DeletionError> {
        self.begin(&Actor::Operator, account).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const OWN: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

    fn config(after: Duration) -> AdminConfig {
        AdminConfig {
            account_delete_batch_size: 50,
            account_delete_interval: Duration::from_millis(200),
            account_delete_alert_after: Duration::from_secs(20),
            account_delete_deadline: after,
            health_check_timeout: None,
            admin_list_page_size: None,
        }
    }

    #[test]
    fn only_the_exact_lowercase_hyphenated_own_id_confirms() {
        // Arrange
        let own = "01234567-89ab-cdef-0123-456789abcdef";
        let cases = [
            (own.to_owned(), true),
            (own.to_uppercase(), false),
            (format!(" {own}"), false),
            (format!("{own} "), false),
            (format!("{own}\n"), false),
            (String::new(), false),
            ("   ".to_owned(), false),
            (OWN.simple().to_string(), false),
            (OWN.braced().to_string(), false),
            (OWN.urn().to_string(), false),
            (Uuid::from_u128(1).to_string(), false),
        ];

        for (confirmation, expected) in cases {
            // Act
            let confirmed = confirms(OWN, &confirmation);

            // Assert
            assert_eq!(confirmed, expected, "{confirmation:?}");
        }
    }

    #[test]
    fn deadline_is_now_plus_the_configured_delay() {
        // Arrange
        let now = Timestamp::from_unix_micros(1_800_000_000_000_000);

        // Act
        let deadline = deadline(now, &config(Duration::from_secs(30)));

        // Assert
        assert_eq!(
            deadline.expect("fits").as_unix_micros(),
            1_800_000_030_000_000
        );
    }

    #[test]
    fn deadline_past_the_timestamp_range_is_an_unexpected_failure() {
        // Arrange
        let now = Timestamp::from_unix_micros(i64::MAX - 1);

        // Act
        let deadline = deadline(now, &config(Duration::from_secs(30)));

        // Assert
        assert!(matches!(deadline, Err(DeletionError::Unexpected(_))));
    }
}
