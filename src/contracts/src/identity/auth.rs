use async_trait::async_trait;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use super::model::{Credential, Principal, Scope};
use crate::common::Fault;

#[derive(Debug, Error)]
pub enum AuthenticatorError {
    /// One variant for a missing, unknown, expired or revoked credential.
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("forbidden")]
    Forbidden,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Resolves a credential to a principal and checks a route's scope.
#[async_trait]
pub trait Authenticator: Send + Sync {
    async fn authenticate(&self, credential: Credential) -> Result<Principal, AuthenticatorError>;

    fn require(&self, principal: &Principal, scope: Scope) -> Result<(), AuthenticatorError>;
}

#[derive(Debug, Error)]
pub enum RevalidatorError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Ends a held long-poll after a revoke. Passed to 05 by 11 as a call parameter.
#[async_trait]
pub trait PrincipalRevalidator: Send + Sync {
    async fn revalidate(&self, principal: &Principal) -> Result<bool, RevalidatorError>;
}

#[derive(Debug, Error)]
pub enum AccountGuardError {
    #[error("account not active")]
    AccountNotActive,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Refuses work for an account that is not active; 02 calls it first inside the commit transaction.
#[async_trait]
pub trait AccountGuard: Send + Sync {
    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountGuardError>;
}
