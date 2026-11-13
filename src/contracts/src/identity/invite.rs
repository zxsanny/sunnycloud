use async_trait::async_trait;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use super::model::{InviteInfo, InviteToken};
use crate::common::Fault;

#[derive(Debug, Error)]
pub enum InviteError {
    /// `revoke` of an unknown, already revoked or already redeemed invite: one answer, so it reveals nothing.
    #[error("not found")]
    NotFound,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait InviteService: Send + Sync {
    /// The plaintext token is returned once and never stored.
    async fn create_invite(
        &self,
        admin_account: Uuid,
        note: &str,
    ) -> Result<InviteToken, InviteError>;

    async fn list(&self, admin_account: Uuid) -> Result<Vec<InviteInfo>, InviteError>;

    async fn revoke(&self, admin_account: Uuid, invite_id: Uuid) -> Result<(), InviteError>;
}

#[derive(Debug, Error)]
pub enum OperatorIdentityError {
    /// `reset_admin` of an unknown account or one being deleted.
    #[error("not found")]
    NotFound,
    /// `reset_admin` of an account whose role is not admin.
    #[error("not an admin")]
    NotAnAdmin,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Identity operations of the operator CLI (system scope).
#[async_trait]
pub trait OperatorIdentityOps: Send + Sync {
    /// `Some` only while no account exists; replaces the open setup token.
    async fn ensure_setup_token(&self) -> Result<Option<RawToken>, OperatorIdentityError>;

    async fn create_admin_invite(&self) -> Result<InviteToken, OperatorIdentityError>;

    /// Revokes the admin's sessions and device tokens and returns an operator invite bound to `account`;
    /// allowed for the only active admin.
    async fn reset_admin(&self, account: Uuid) -> Result<InviteToken, OperatorIdentityError>;
}
