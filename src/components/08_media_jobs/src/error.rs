use sc_error::{BusinessError, BusinessParams, DomainError, ErrorCode};
use thiserror::Error;

/// Expected failures of the media operations (tier 2). Each variant has one stable code and status in the
/// `sc_error` table; none carries a job id, an account id or any request data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MediaError {
    #[error("lease lost")]
    LeaseLost,
    #[error("revision obsolete")]
    RevisionObsolete,
    /// The derived file is not produced yet (a job is queued or running).
    #[error("derived file not ready")]
    NotReady,
    /// No derived file exists for the content and none will be made.
    #[error("derived file not found")]
    NotFound,
    /// Every authentication failure of the worker token, whatever the cause.
    #[error("forbidden")]
    Forbidden,
    #[error("output too large")]
    OutputTooLarge,
    #[error("output type rejected")]
    OutputTypeRejected,
    #[error("output kind rejected")]
    OutputKindRejected,
}

impl MediaError {
    pub fn code(self) -> ErrorCode {
        match self {
            MediaError::LeaseLost => ErrorCode::LeaseLost,
            MediaError::RevisionObsolete => ErrorCode::RevisionObsolete,
            MediaError::NotReady => ErrorCode::ThumbnailNotReady,
            MediaError::NotFound => ErrorCode::ThumbnailUnavailable,
            MediaError::Forbidden => ErrorCode::Forbidden,
            MediaError::OutputTooLarge => ErrorCode::OutputTooLarge,
            MediaError::OutputTypeRejected => ErrorCode::OutputTypeRejected,
            MediaError::OutputKindRejected => ErrorCode::OutputKindRejected,
        }
    }
}

impl From<MediaError> for BusinessError {
    fn from(error: MediaError) -> BusinessError {
        BusinessError::new(error.code(), BusinessParams::default())
    }
}

impl From<MediaError> for DomainError {
    fn from(error: MediaError) -> DomainError {
        DomainError::Business(error.into())
    }
}
