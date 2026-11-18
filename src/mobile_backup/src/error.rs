//! `BackupError`: the typed failures of `BackupCore` (`backup_core.md` § Types). Texts are fixed, except the server's
//! upgrade message; no token is ever part of an error.

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum BackupError {
    #[error("not signed in")]
    NotSignedIn,
    #[error("server unreachable")]
    ServerUnreachable,
    /// `message` is the server's text for the user.
    #[error("client upgrade required: {message}")]
    UpgradeRequired { message: String },
    #[error("token rejected")]
    TokenRejected,
    #[error("invalid code")]
    InvalidCode,
    #[error("https required")]
    HttpsRequired,
    /// The address is not an absolute `https` URL with a host (backup_core.md 1.1.0).
    #[error("invalid server url")]
    InvalidServerUrl,
    /// The checkpoint database could not be read (backup_core.md 1.1.0); the cause is logged.
    #[error("checkpoint unavailable")]
    CheckpointUnavailable,
}
