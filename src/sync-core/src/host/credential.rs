use std::fmt;

use sc_config::Secret;
use thiserror::Error;

/// What the host holds for the signed-in user. `refresh = None` is a desktop device token: it is never refreshed.
///
/// Both values are `Secret`s, so no formatting of a credential shows them.
#[derive(Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Credential {
    pub access: Secret<String>,
    pub refresh: Option<Secret<String>>,
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl fmt::Display for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// The host could not store a rotated token pair durably.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("credential persist failed")]
pub struct PersistFailed;

/// The host holds no credential it can give now: its store is locked, unreadable or past its time bound, or no
/// token is stored. The core sends nothing and answers `SyncError::CredentialUnavailable`; it never signs out for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("credential unavailable")]
pub struct CredentialUnavailable;

/// Credentials of the signed-in user, owned by the host.
pub trait CredentialProvider: Send + Sync {
    /// Called before every request; the core drops the value when that request ends.
    fn bearer(&self) -> Result<Credential, CredentialUnavailable>;

    /// Returns only after the pair is durably stored. No request uses `access` before this returned `Ok`.
    fn on_rotated(
        &self,
        access: Secret<String>,
        refresh: Secret<String>,
    ) -> Result<(), PersistFailed>;
}
