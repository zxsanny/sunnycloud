//! `InviteService` (member invites, called by 11 for a signed-in admin) and `OperatorIdentityOps` (the setup
//! token, the operator admin invite and the admin reset, called by 12 in system scope), contract
//! `operator_identity_ops.md`. Every token is returned in plaintext once, by the call that creates it; only its
//! SHA-256 is stored, and nothing here logs it. Redemption is part of sign-in (`registration_repo`).

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    Fault, InviteError, InviteInfo, InviteService, InviteToken, OperatorIdentityError,
    OperatorIdentityOps, async_trait,
};
use sc_audit::AuditWriter;
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, RunError};
use sc_ids::{RawToken, TokenKind, new_id, new_token, sha256_token};
use uuid::Uuid;

use crate::auth_repo;
use crate::config::IdentityConfig;
use crate::error::IdentityError;
use crate::invite_repo::{InviteKind, ListedInvite, NewInvite};
use crate::operator_repo::ResetError;

const USER: &str = "user";
const ADMIN: &str = "admin";

/// What a new token row grants and to whom it belongs, besides its token and times.
struct Grant<'a> {
    kind: InviteKind,
    grants_role: &'static str,
    target_account_id: Option<Uuid>,
    created_by: Option<Uuid>,
    note: Option<&'a str>,
}

/// A fresh token and its row, not stored yet.
struct Issued<'a> {
    token: RawToken,
    row: NewInvite<'a>,
}

/// Invites and the operator's identity operations over the identity tables.
pub struct IdentityInvites {
    db: Db,
    clock: Arc<dyn Clock>,
    audit: AuditWriter,
    invite_ttl: Duration,
    setup_token_ttl: Duration,
}

impl IdentityInvites {
    /// Opens no connection. `config` is the validated identity section (`invite_ttl`, `setup_token_ttl`).
    pub fn new(db: Db, clock: Arc<dyn Clock>, config: &IdentityConfig, audit: AuditWriter) -> Self {
        IdentityInvites {
            db,
            clock,
            audit,
            invite_ttl: config.invite_ttl,
            setup_token_ttl: config.setup_token_ttl,
        }
    }

    /// A new token of `grant`, valid from now for `ttl`.
    fn issue<'a>(&self, grant: Grant<'a>, ttl: Duration) -> Result<Issued<'a>, Fault> {
        let token_kind = match grant.kind {
            InviteKind::Invite => TokenKind::Invite,
            InviteKind::Setup => TokenKind::Setup,
        };
        let token = new_token(token_kind).map_err(Fault::from)?;
        let created_at = self.clock.now_utc();
        let expires_at = created_at
            .checked_add(ttl)
            .ok_or_else(|| Fault::from(IdentityError::TimeOutOfRange))?;
        let row = NewInvite {
            id: new_id(self.clock.as_ref()),
            kind: grant.kind,
            token_hash: sha256_token(token.expose()),
            grants_role: grant.grants_role,
            target_account_id: grant.target_account_id,
            created_by: grant.created_by,
            note: grant.note,
            created_at,
            expires_at,
        };
        Ok(Issued { token, row })
    }

    /// An invite that creates an account with `role` on redemption.
    async fn create(
        &self,
        role: &'static str,
        created_by: Option<Uuid>,
        note: Option<&str>,
    ) -> Result<InviteToken, Fault> {
        let issued = self.issue(
            Grant {
                kind: InviteKind::Invite,
                grants_role: role,
                target_account_id: None,
                created_by,
                note,
            },
            self.invite_ttl,
        )?;
        auth_repo::insert_invite(&self.db, &issued.row)
            .await
            .map_err(Fault::from)?;
        Ok(InviteToken(issued.token))
    }
}

fn invite_unexpected(error: impl std::error::Error + Send + Sync + 'static) -> InviteError {
    InviteError::Unexpected(Box::new(error))
}

fn operator_unexpected(
    error: impl std::error::Error + Send + Sync + 'static,
) -> OperatorIdentityError {
    OperatorIdentityError::Unexpected(Box::new(error))
}

fn invite_info(row: ListedInvite) -> InviteInfo {
    InviteInfo {
        id: row.id,
        note: row.note,
        created_at: Timestamp::from_unix_micros(row.created_at_micros),
        expires_at: Timestamp::from_unix_micros(row.expires_at_micros),
        redeemed_at: row.redeemed_at_micros.map(Timestamp::from_unix_micros),
    }
}

#[async_trait]
impl InviteService for IdentityInvites {
    /// A member invite (`invite_ttl`) created by `admin_account`; the caller (11) has authorized the admin.
    async fn create_invite(
        &self,
        admin_account: Uuid,
        note: &str,
    ) -> Result<InviteToken, InviteError> {
        self.create(USER, Some(admin_account), Some(note))
            .await
            .map_err(InviteError::Unexpected)
    }

    /// Member and operator admin invites, newest first, redeemed ones included; never a token.
    async fn list(&self, _admin_account: Uuid) -> Result<Vec<InviteInfo>, InviteError> {
        let rows = auth_repo::list_invites(&self.db)
            .await
            .map_err(invite_unexpected)?;
        Ok(rows.into_iter().map(invite_info).collect())
    }

    /// Deletes the open invite. An unknown, already revoked or redeemed invite gives one and the same failure,
    /// `NotFound`, so the call reveals nothing about it.
    async fn revoke(&self, _admin_account: Uuid, invite_id: Uuid) -> Result<(), InviteError> {
        if auth_repo::revoke_invite(&self.db, invite_id)
            .await
            .map_err(invite_unexpected)?
        {
            Ok(())
        } else {
            Err(InviteError::NotFound)
        }
    }
}

#[async_trait]
impl OperatorIdentityOps for IdentityInvites {
    async fn ensure_setup_token(&self) -> Result<Option<RawToken>, OperatorIdentityError> {
        let issued = self
            .issue(
                Grant {
                    kind: InviteKind::Setup,
                    grants_role: ADMIN,
                    target_account_id: None,
                    created_by: None,
                    note: None,
                },
                self.setup_token_ttl,
            )
            .map_err(OperatorIdentityError::Unexpected)?;
        let stored = auth_repo::replace_setup_token(&self.db, &issued.row)
            .await
            .map_err(operator_unexpected)?;
        Ok(stored.then_some(issued.token))
    }

    async fn create_admin_invite(&self) -> Result<InviteToken, OperatorIdentityError> {
        self.create(ADMIN, None, None)
            .await
            .map_err(OperatorIdentityError::Unexpected)
    }

    /// Allowed for the only active admin (G-12-3). An unknown account or one being deleted is `NotFound`, one
    /// that is not an admin `NotAnAdmin`; both change nothing.
    async fn reset_admin(&self, account: Uuid) -> Result<InviteToken, OperatorIdentityError> {
        let issued = self
            .issue(
                Grant {
                    kind: InviteKind::Invite,
                    grants_role: ADMIN,
                    target_account_id: Some(account),
                    created_by: None,
                    note: None,
                },
                self.invite_ttl,
            )
            .map_err(OperatorIdentityError::Unexpected)?;
        let now = issued.row.created_at;
        match auth_repo::reset_admin(&self.db, account, &issued.row, now, &self.audit).await {
            Ok(()) => Ok(InviteToken(issued.token)),
            Err(RunError::Closure(ResetError::AccountNotFound | ResetError::Deleting)) => {
                Err(OperatorIdentityError::NotFound)
            }
            Err(RunError::Closure(ResetError::NotAnAdmin)) => {
                Err(OperatorIdentityError::NotAnAdmin)
            }
            Err(other) => Err(operator_unexpected(other)),
        }
    }
}
