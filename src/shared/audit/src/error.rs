use sc_db::DbError;
use thiserror::Error;

/// Why the shape check refused an event before any statement ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Rejection {
    /// The detail renders to more than [`crate::MAX_DETAIL_BYTES`] bytes (the database `CHECK` has the same limit).
    #[error("audit detail is {bytes} bytes, the limit is {limit}", limit = crate::MAX_DETAIL_BYTES)]
    DetailTooLarge { bytes: usize },
    /// A key, enumeration code or subject type is not 1 to 64 characters of `a-z`, `0-9` and `_`. `place` names
    /// which one; the offending text is never repeated.
    #[error("{place} is not a lower-case identifier of 1 to 64 characters")]
    InvalidToken { place: &'static str },
    /// Two detail values share a key; `jsonb` would keep one and drop the other.
    #[error("audit detail repeats a key")]
    DuplicateKey,
}

/// Failure of a writer, purge or export call.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuditError {
    /// The event has the wrong shape; nothing was inserted.
    #[error("audit event rejected: {0}")]
    Rejected(#[from] Rejection),
    /// The database refused or lost the statement. A caller inside a business transaction returns this error so
    /// the transaction rolls back.
    #[error("audit store failure: {0}")]
    Store(#[from] DbError),
}
