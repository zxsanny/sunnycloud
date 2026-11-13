//! The operator's identity operations on the system-scope transaction `auth_repo` opens (contract
//! `operator_identity_ops.md`): issuing the setup token while no account exists, and the admin reset that cuts
//! every way into the account and leaves only the re-bind invite it issues.

use sc_audit::{AuditError, AuditWriter};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use thiserror::Error;
use uuid::Uuid;

use crate::account_repo;
use crate::audit_events;
use crate::device_grant_repo;
use crate::invite_repo::{self, NewInvite};
use crate::login_code_repo;
use crate::session_repo;

const ADMIN: &str = "admin";
const DELETING: &str = "deleting";
/// The stored revoke reason of the reset's sessions and device tokens.
const REVOKE_REASON: &str = "admin";

/// Why an admin reset rolled back.
#[derive(Debug, Error)]
pub(crate) enum ResetError {
    #[error("account not found")]
    AccountNotFound,
    #[error("the account is not an admin")]
    NotAnAdmin,
    #[error("the account is being deleted")]
    Deleting,
    /// A mandatory audit event was not written; nothing was changed.
    #[error("the mandatory audit event was not written")]
    Audit(#[source] AuditError),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Stores `setup` as the only open setup token, replacing any earlier one; `false` (and nothing written) when an
/// account exists.
pub(crate) async fn replace_setup_token(
    tx: &mut DbTx,
    setup: &NewInvite<'_>,
) -> Result<bool, DbError> {
    if account_repo::any_account(tx).await? {
        return Ok(false);
    }
    invite_repo::delete_open_setup(tx).await?;
    invite_repo::insert(tx, setup).await?;
    Ok(true)
}

/// Resets admin `account` and stores `rebind` (its target is `account`): sessions and device tokens revoked,
/// approved grants not yet collected denied, unexchanged mobile login codes deleted, the identities removed (so
/// nobody signs in as the account until the invite is redeemed), earlier open re-bind invites deleted. The
/// account keeps its role, state and data. The mandatory `session_revoked` (when a session was live) and
/// `admin_reset` events are the last statements.
pub(crate) async fn reset_admin(
    tx: &mut DbTx,
    account: Uuid,
    rebind: &NewInvite<'_>,
    now: Timestamp,
    audit: &AuditWriter,
) -> Result<(), ResetError> {
    let (role, state) = account_repo::lock_role_state(tx, account)
        .await?
        .ok_or(ResetError::AccountNotFound)?;
    if role != ADMIN {
        return Err(ResetError::NotAnAdmin);
    }
    if state == DELETING {
        return Err(ResetError::Deleting);
    }
    let revoked = session_repo::revoke_all(tx, account, now, REVOKE_REASON).await?;
    device_grant_repo::revoke_all_devices(tx, account, now, REVOKE_REASON).await?;
    device_grant_repo::deny_open_grants(tx, account).await?;
    login_code_repo::delete_for_account(tx, account).await?;
    account_repo::delete_identities(tx, account).await?;
    invite_repo::delete_open_rebind(tx, account).await?;
    invite_repo::insert(tx, rebind).await?;
    if revoked > 0 {
        let event = audit_events::session_revoked(account, None, REVOKE_REASON);
        audit
            .record_in(tx, event)
            .await
            .map_err(ResetError::Audit)?;
    }
    let event = audit_events::admin_reset(account, rebind.id);
    audit.record_in(tx, event).await.map_err(ResetError::Audit)
}
