use async_trait::async_trait;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use super::model::{Principal, RevokeReason, SessionInfo, SessionTokens};
use crate::common::Fault;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("refresh token reuse detected")]
    ReuseDetected,
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn refresh(&self, refresh_token: &RawToken) -> Result<SessionTokens, SessionError>;

    async fn revoke(
        &self,
        account: Uuid,
        session_id: Uuid,
        reason: RevokeReason,
    ) -> Result<(), SessionError>;

    async fn revoke_all(&self, account: Uuid, reason: RevokeReason) -> Result<(), SessionError>;

    async fn list(&self, account: Uuid) -> Result<Vec<SessionInfo>, SessionError>;

    /// Records activity; the provider writes `last_seen_at` at most once a minute.
    async fn touch(&self, principal: &Principal) -> Result<(), SessionError>;
}
