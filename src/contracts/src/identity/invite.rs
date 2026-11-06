use async_trait::async_trait;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use super::model::{InviteInfo, InviteToken};
use crate::common::Fault;

#[derive(Debug, Error)]
pub enum InviteError {
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
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Identity operations of the operator CLI (system scope).
#[async_trait]
pub trait OperatorIdentityOps: Send + Sync {
    /// `Some` only while no account exists; replaces the open setup token.
    async fn ensure_setup_token(&self) -> Result<Option<RawToken>, OperatorIdentityError>;

    async fn create_admin_invite(&self) -> Result<InviteToken, OperatorIdentityError>;

    /// Revokes the admin's sessions and returns an operator invite bound to `account`.
    async fn reset_admin(&self, account: Uuid) -> Result<InviteToken, OperatorIdentityError>;
}
