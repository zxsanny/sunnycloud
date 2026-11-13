//! Redemption of the token presented by an unknown identity, on the one system-scope transaction `auth_repo`
//! opens (contract `account_creation_hooks.md`, `operator_identity_ops.md`):
//!
//! - a member or operator admin invite, and the setup token while no account exists, create an account: the
//!   `accounts` and `identities` rows, `initialize` of every hook in registration order, the conditional
//!   redemption, then the mandatory `account_created` event as the last statement;
//! - a re-bind invite (`target_account_id` set) creates no account and calls no hook: under the target's row
//!   lock it is redeemed, the target's identities are replaced by the signing-in one, and the mandatory
//!   `identity_rebound` event is written.
//!
//! Any failure rolls all of it back, so the token stays redeemable.

use std::sync::Arc;

use contracts::{AccountLifecycleHook, LifecycleError};
use sc_audit::{AuditError, AuditWriter};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use thiserror::Error;
use uuid::Uuid;

use crate::account_repo::{self, NewIdentity};
use crate::audit_events;
use crate::invite_repo::{self, OpenToken};

/// An unknown identity that presented a token: the rows identity writes and what the provider said about the
/// user. `account_id` and `identity.account_id` are the id of the account a creating token makes; a re-bind
/// invite uses its target instead.
pub(crate) struct NewRegistration<'a> {
    pub invite_token_hash: &'a [u8],
    pub account_id: Uuid,
    pub display_name: &'a str,
    pub identity: NewIdentity<'a>,
    pub now: Timestamp,
}

/// What a redemption did.
pub(crate) enum Redeemed {
    /// A new active account with id `NewRegistration::account_id`.
    Created,
    /// The identity now belongs to the existing account `account`, whose state is `state`.
    Rebound { account: Uuid, state: String },
}

/// Why the redemption transaction rolled back.
#[derive(Debug, Error)]
pub(crate) enum CreateError {
    /// No open token for the hash, a setup token while an account exists, or a concurrent redemption won.
    #[error("the invite is not redeemable")]
    InviteNotRedeemable,
    /// An account would be created while the composition root has not registered the hook list.
    #[error("account lifecycle hooks are not registered")]
    HooksNotRegistered,
    #[error("an account lifecycle hook failed")]
    Hook(#[source] LifecycleError),
    /// A mandatory audit event was not written; nothing was created or bound.
    #[error("the mandatory audit event was not written")]
    Audit(#[source] AuditError),
    /// The invite carries a role outside the `invites_grants_role` CHECK; a damaged database.
    #[error("unexpected invite role")]
    UnexpectedRole,
    #[error(transparent)]
    Db(#[from] DbError),
}

/// The audit code of an invite's `grants_role`.
fn role_code(role: &str) -> Option<&'static str> {
    match role {
        "user" => Some("user"),
        "admin" => Some("admin"),
        _ => None,
    }
}

/// Redeems the token of `registration`. `hooks` is `None` while the list is unregistered: only a re-bind can
/// then succeed, a creation fails closed.
pub(crate) async fn redeem(
    tx: &mut DbTx,
    registration: &NewRegistration<'_>,
    hooks: Option<&[Arc<dyn AccountLifecycleHook>]>,
    audit: &AuditWriter,
) -> Result<Redeemed, CreateError> {
    let token = invite_repo::open_token(tx, registration.invite_token_hash, registration.now)
        .await?
        .ok_or(CreateError::InviteNotRedeemable)?;
    match token.target_account_id {
        Some(target) => rebind(tx, registration, token.id, target, audit).await,
        None => {
            let hooks = hooks.ok_or(CreateError::HooksNotRegistered)?;
            create(tx, registration, &token, hooks, audit).await?;
            Ok(Redeemed::Created)
        }
    }
}

async fn create(
    tx: &mut DbTx,
    registration: &NewRegistration<'_>,
    token: &OpenToken,
    hooks: &[Arc<dyn AccountLifecycleHook>],
    audit: &AuditWriter,
) -> Result<(), CreateError> {
    let (account, now) = (registration.account_id, registration.now);
    // The setup token bootstraps the first account only. Two concurrent setup redemptions both pass this read;
    // the conditional redemption below lets one of them commit.
    if token.is_setup && account_repo::any_account(tx).await? {
        return Err(CreateError::InviteNotRedeemable);
    }
    account_repo::insert_account(
        tx,
        account,
        registration.display_name,
        &token.grants_role,
        now,
    )
    .await?;
    account_repo::insert_identity(tx, &registration.identity).await?;
    for hook in hooks {
        hook.initialize(tx, account)
            .await
            .map_err(CreateError::Hook)?;
    }
    if !invite_repo::redeem(tx, token.id, account, now).await? {
        return Err(CreateError::InviteNotRedeemable);
    }
    let role = role_code(&token.grants_role).ok_or(CreateError::UnexpectedRole)?;
    let event = audit_events::account_created(account, registration.identity.id, token.id, role);
    audit.record_in(tx, event).await.map_err(CreateError::Audit)
}

/// Binds the signing-in identity to `target`. The row lock orders concurrent re-binds of one account; the
/// conditional redemption decides which of them commits.
async fn rebind(
    tx: &mut DbTx,
    registration: &NewRegistration<'_>,
    invite: Uuid,
    target: Uuid,
    audit: &AuditWriter,
) -> Result<Redeemed, CreateError> {
    let now = registration.now;
    let state = account_repo::lock(tx, target)
        .await?
        .ok_or(CreateError::InviteNotRedeemable)?;
    if !invite_repo::redeem(tx, invite, target, now).await? {
        return Err(CreateError::InviteNotRedeemable);
    }
    account_repo::delete_identities(tx, target).await?;
    let identity = NewIdentity {
        account_id: target,
        ..registration.identity
    };
    account_repo::insert_identity(tx, &identity).await?;
    let event = audit_events::identity_rebound(target, identity.id, invite);
    audit
        .record_in(tx, event)
        .await
        .map_err(CreateError::Audit)?;
    Ok(Redeemed::Rebound {
        account: target,
        state,
    })
}
