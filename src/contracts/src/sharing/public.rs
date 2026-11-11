use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;
use crate::tree::LiveFile;

/// Result of opening a link. `NotFound` is the one outcome for an unknown, expired, revoked, tombstoned or
/// purged link, with no reason field (AC-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareOpen {
    Available {
        name: String,
        size: i64,
        media_type: Option<String>,
        expires_at: Timestamp,
    },
    PasswordRequired,
    NotFound,
}

/// Short-lived proof of a correct password, bound to one link. `Debug` prints a placeholder.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessGrant {
    pub value: String,
    pub expires_at: Timestamp,
}

impl fmt::Debug for AccessGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessGrant")
            .field("value", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// The current revision of the shared file, for the caller to stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareContent {
    pub account_id: Uuid,
    pub node_id: Uuid,
    pub revision_id: Uuid,
}

/// The shared file for a derived-file route: the owning account (for the lookup and for 03) with the file that
/// `DerivedOpener::open_derived_for` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareDerived {
    pub account_id: Uuid,
    pub file: LiveFile,
}

#[derive(Debug, Error)]
pub enum PublicShareError {
    /// One variant for every missing, expired or revoked link (AC-12).
    #[error("not found")]
    NotFound,
    #[error("wrong password")]
    WrongPassword,
    #[error("password required")]
    PasswordRequired,
    #[error("rate limited")]
    RateLimited { retry_after: Duration },
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Visitor-side access decisions; it never serves bytes.
#[async_trait]
pub trait PublicShareService: Send + Sync {
    async fn open(
        &self,
        token: &RawToken,
        client_ip: IpAddr,
    ) -> Result<ShareOpen, PublicShareError>;

    async fn verify_password(
        &self,
        token: &RawToken,
        password: &str,
        client_ip: IpAddr,
    ) -> Result<AccessGrant, PublicShareError>;

    /// Re-checks the link and its target on every call. `starts_download` is set by 11 when the request has no
    /// `Range` or its range starts at byte 0; only then is `share_accessed` audited.
    async fn authorize_content(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
        starts_download: bool,
    ) -> Result<ShareContent, PublicShareError>;

    /// As `authorize_content` for a derived file; `file` is the argument of `DerivedOpener::open_derived_for` and
    /// `account_id` is the account 11 passes to 03 for the derived blob.
    async fn authorize_derived(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<ShareDerived, PublicShareError>;
}
