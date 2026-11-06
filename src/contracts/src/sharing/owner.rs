use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

/// `token` is the plaintext link secret and exists only here; its `Debug` prints a placeholder.
#[derive(Debug)]
pub struct CreatedShare {
    pub id: Uuid,
    pub token: RawToken,
    pub url_path: String,
    pub expires_at: Timestamp,
    pub has_password: bool,
}

/// A link as listed to its owner. It never carries the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareInfo {
    pub id: Uuid,
    pub node_id: Uuid,
    pub expires_at: Timestamp,
    pub has_password: bool,
    pub created_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum ShareError {
    /// The node is gone, belongs to another account or never existed.
    #[error("target not found")]
    TargetNotFound,
    /// The target is a folder (or, in v1.2, a vault file).
    #[error("target not shareable")]
    TargetNotShareable,
    #[error("expiry out of range")]
    ExpiryOutOfRange,
    #[error("not found")]
    NotFound,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Owner-side link lifecycle; session-only at the HTTP boundary.
#[async_trait]
pub trait ShareService: Send + Sync {
    /// `expires_in` defaults to `share_default_ttl`.
    async fn create(
        &self,
        account: Uuid,
        node_id: Uuid,
        expires_in: Option<Duration>,
        password: Option<&str>,
    ) -> Result<CreatedShare, ShareError>;

    async fn list(
        &self,
        account: Uuid,
        node_id: Option<Uuid>,
    ) -> Result<Vec<ShareInfo>, ShareError>;

    /// Idempotent for the owner's own link.
    async fn revoke(&self, account: Uuid, share_id: Uuid) -> Result<(), ShareError>;
}
