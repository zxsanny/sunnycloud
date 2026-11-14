use sc_audit::AuditError;
use sc_db::DbError;
use thiserror::Error;

/// Failures of the sharing component that are not port outcomes; they travel as the `Unexpected` fault.
#[derive(Debug, Error)]
pub enum SharingError {
    /// The operating system's random source failed; no token or salt can be drawn.
    #[error("random source unavailable")]
    RandomUnavailable,
    /// Argon2id refused to hash a link password, or a stored PHC string did not parse; the password is never part
    /// of the message.
    #[error("link password hashing failed: {0}")]
    PasswordHash(argon2::password_hash::Error),
    /// The blocking task that hashes a link password panicked or was cancelled.
    #[error("link password hashing did not finish")]
    HashTask(#[source] tokio::task::JoinError),
    /// The bound on password checks at once was closed; no check can start.
    #[error("the password check pool is closed")]
    PasswordPool(#[source] tokio::sync::AcquireError),
    /// The clock plus a lifetime leaves the representable range.
    #[error("the link lifetime is out of the clock's range")]
    TimeOutOfRange,
}

/// The failure of a transaction that writes a mandatory audit event: its statements or the audit insert.
#[derive(Debug, Error)]
pub(crate) enum TxError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Audit(#[from] AuditError),
}
