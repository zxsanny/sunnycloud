//! The purger's steps. Each runs in its own short transaction in the account's scope, which first takes the
//! claim through `AccountAdmin::lock_deleting`; a step whose claim fails changes nothing and is skipped. The
//! claim transaction lives for one bounded hook batch, or for the finalize and its audit row.

use std::fmt;

use contracts::{
    AccountAdmin, AccountAdminError, AccountLifecycleHook, EraseProgress, LifecycleError,
};
use sc_audit::{AuditError, AuditEvent, AuditEventKind, AuditWriter, SubjectRef};
use sc_db::{Db, DbError, RunError, Scope};
use thiserror::Error;
use uuid::Uuid;

use crate::user_admin::SUBJECT_TYPE;

/// What a step did: its work, or nothing because the claim was not taken.
pub(crate) enum Claimed<T> {
    Done(T),
    Skipped,
}

/// The cause of a failed step; it carries no row data.
#[derive(Debug, Error)]
pub(crate) enum StepError {
    #[error("the claim on the account row failed")]
    Claim(#[source] AccountAdminError),
    #[error("the erase batch failed")]
    Hook(#[source] LifecycleError),
    #[error("finalize_deletion failed")]
    Finalize(#[source] AccountAdminError),
    #[error("the account_deleted audit row was not written")]
    Audit(#[source] AuditError),
    #[error("the step transaction failed")]
    Db(#[source] DbError),
}

/// Where in the walk a step failed: a hook by its 0-based position in the registered list, or the finalize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepAt {
    Hook(usize),
    Finalize(usize),
}

impl fmt::Display for StepAt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepAt::Hook(position) => write!(f, "hook position {position}"),
            StepAt::Finalize(position) => write!(f, "finalize (position {position})"),
        }
    }
}

/// The ERROR line's error: the position and the cause chain; the account id is the line's own field.
#[derive(Debug, Error)]
#[error("account erase step at {at} failed")]
pub(crate) struct StepFailure {
    pub at: StepAt,
    #[source]
    pub cause: StepError,
}

fn settled<T>(result: Result<T, RunError<StepError>>) -> Result<T, StepError> {
    result.map_err(|error| match error {
        RunError::Closure(error) => error,
        RunError::Db(error) => StepError::Db(error),
    })
}

/// One `erase_batch` of `hook` under the claim.
pub(crate) async fn erase_step(
    db: &Db,
    accounts: &dyn AccountAdmin,
    hook: &dyn AccountLifecycleHook,
    account: Uuid,
    limit: u32,
) -> Result<Claimed<EraseProgress>, StepError> {
    settled(
        db.run(Scope::Account(account), async |tx| {
            if !accounts
                .lock_deleting(tx, account)
                .await
                .map_err(StepError::Claim)?
            {
                return Ok(Claimed::Skipped);
            }
            hook.erase_batch(account, limit)
                .await
                .map(Claimed::Done)
                .map_err(StepError::Hook)
        })
        .await,
    )
}

/// `finalize_deletion` and the `account_deleted` audit row in one transaction under the claim. The claim fails
/// once the account is gone, so a second purger never writes a second audit row.
pub(crate) async fn finalize_step(
    db: &Db,
    accounts: &dyn AccountAdmin,
    audit: &AuditWriter,
    account: Uuid,
) -> Result<Claimed<()>, StepError> {
    settled(
        db.run(Scope::Account(account), async |tx| {
            if !accounts
                .lock_deleting(tx, account)
                .await
                .map_err(StepError::Claim)?
            {
                return Ok(Claimed::Skipped);
            }
            accounts
                .finalize_deletion(tx, account)
                .await
                .map_err(StepError::Finalize)?;
            audit
                .record_in(tx, account_deleted(account))
                .await
                .map_err(StepError::Audit)?;
            Ok(Claimed::Done(()))
        })
        .await,
    )
}

fn account_deleted(account: Uuid) -> AuditEvent {
    AuditEvent {
        subject: Some(SubjectRef {
            subject_type: SUBJECT_TYPE,
            id: account,
        }),
        ..AuditEvent::new(AuditEventKind::AccountDeleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_names_its_position_and_no_row_data() {
        // Arrange
        let hook = StepFailure {
            at: StepAt::Hook(2),
            cause: StepError::Hook(LifecycleError::Unexpected("injected".into())),
        };
        let finalize = StepFailure {
            at: StepAt::Finalize(5),
            cause: StepError::Finalize(AccountAdminError::Unexpected("injected".into())),
        };

        // Act
        let texts = [hook.to_string(), finalize.to_string()];

        // Assert
        assert_eq!(
            texts,
            [
                "account erase step at hook position 2 failed",
                "account erase step at finalize (position 5) failed"
            ]
        );
    }

    #[test]
    fn the_audit_entry_is_account_deleted_about_the_account_only() {
        // Arrange
        let account = Uuid::from_u128(7);

        // Act
        let event = account_deleted(account);

        // Assert
        assert_eq!(event.event, AuditEventKind::AccountDeleted);
        assert_eq!(event.subject.map(|subject| subject.id), Some(account));
        assert_eq!(event.actor_account_id, None);
        assert!(event.detail.is_empty());
    }
}
