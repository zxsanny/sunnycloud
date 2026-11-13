use sc_db::DbError;
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};
use thiserror::Error;

/// The failures of a tree operation. The first ten are the expected business outcomes with a stable wire code;
/// `Database` is the unexpected tier, kept as a value so the retry rule can read its class.
#[derive(Debug, Error)]
pub enum TreeError {
    #[error("name conflict")]
    NameConflict,
    #[error("invalid name")]
    InvalidName,
    #[error("cycle")]
    Cycle,
    #[error("edited since")]
    EditedSince,
    #[error("parent deleted")]
    ParentDeleted,
    #[error("parent not found")]
    ParentNotFound,
    #[error("not a folder")]
    NotAFolder,
    #[error("invalid cursor")]
    InvalidCursor,
    #[error("batch too large")]
    BatchTooLarge,
    #[error("not found")]
    NotFound,
    #[error("database failure")]
    Database(#[from] DbError),
}

impl TreeError {
    /// The wire code of this failure in the shared table of `sc_error`; `Database` is `internal`.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            TreeError::NameConflict => ErrorCode::NameConflict,
            TreeError::InvalidName => ErrorCode::InvalidName,
            TreeError::Cycle => ErrorCode::Cycle,
            TreeError::EditedSince => ErrorCode::EditedSince,
            TreeError::ParentDeleted => ErrorCode::ParentDeleted,
            TreeError::ParentNotFound => ErrorCode::ParentNotFound,
            TreeError::NotAFolder => ErrorCode::NotAFolder,
            TreeError::InvalidCursor => ErrorCode::InvalidCursor,
            TreeError::BatchTooLarge => ErrorCode::BatchTooLarge,
            TreeError::NotFound => ErrorCode::NotFound,
            TreeError::Database(_) => ErrorCode::Internal,
        }
    }

    /// Tier 2 for a business outcome; tier 3 for a database failure, whose text stays in the log source and
    /// never reaches a response.
    pub fn into_domain(self, correlation_id: CorrelationId) -> DomainError {
        match self {
            TreeError::Database(error) => DomainError::unexpected(error, correlation_id),
            business => DomainError::Business(BusinessError::new(
                business.error_code(),
                BusinessParams::default(),
            )),
        }
    }
}
