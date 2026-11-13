use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

/// `token` is the plaintext link secret and exists only here. `url_path` carries the token too, so `Debug` prints
/// neither: both show a placeholder.
pub struct CreatedShare {
    pub id: Uuid,
    pub token: RawToken,
    pub url_path: String,
    pub expires_at: Timestamp,
    pub has_password: bool,
}

impl std::fmt::Debug for CreatedShare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedShare")
            .field("id", &self.id)
            .field("token", &self.token)
            .field("url_path", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("has_password", &self.has_password)
            .finish()
    }
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
