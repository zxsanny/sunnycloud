use std::time::Duration;

use sc_error::{BusinessError, BusinessParams, DomainError, ErrorCode};
use thiserror::Error;

/// Expected failures of the transfer operations (tier 2). Each variant has one stable code and status in the
/// `sc_error` table; none carries a name, a hash or any request data.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransferError {
    #[error("quota exceeded")]
    QuotaExceeded,
    #[error("free space floor reached")]
    FreeSpaceFloor,
    #[error("size too large")]
    SizeTooLarge,
    #[error("upload session cap exceeded")]
    SessionCapExceeded,
    #[error("staged file cap reached")]
    StagedCapReached,
    #[error("commit id reused")]
    CommitIdReused,
    #[error("batch too large")]
    BatchTooLarge,
    #[error("blocklist mismatch")]
    BlocklistMismatch,
    #[error("account not active")]
    AccountNotActive,
    #[error("parent not found")]
    ParentNotFound,
    #[error("not a folder")]
    NotAFolder,
    #[error("parent deleted")]
    ParentDeleted,
    #[error("block hash mismatch")]
    BlockHashMismatch,
    #[error("block length mismatch")]
    BlockLengthMismatch,
    #[error("block conflict")]
    BlockConflict,
    /// The indexes of the blocks the session still lacks.
    #[error("blocks needed")]
    NeedBlocks(Vec<u32>),
    #[error("edited since")]
    EditedSince,
    #[error("content unavailable")]
    ContentUnavailable,
    #[error("range not satisfiable")]
    RangeNotSatisfiable,
    #[error("upload session gone")]
    UploadSessionGone,
    /// A global buffer, lane or account in-flight cap; the delivery layer sends `retry_after` as `Retry-After`.
    #[error("overloaded")]
    Overloaded { retry_after: Duration },
}

impl TransferError {
    pub fn code(&self) -> ErrorCode {
        match self {
            TransferError::QuotaExceeded => ErrorCode::QuotaExceeded,
            TransferError::FreeSpaceFloor => ErrorCode::FreeSpaceFloor,
            TransferError::SizeTooLarge => ErrorCode::SizeTooLarge,
            TransferError::SessionCapExceeded => ErrorCode::SessionCapExceeded,
            TransferError::StagedCapReached => ErrorCode::StagedCapReached,
            TransferError::CommitIdReused => ErrorCode::CommitIdReused,
            TransferError::BatchTooLarge => ErrorCode::BatchTooLarge,
            TransferError::BlocklistMismatch => ErrorCode::BlocklistMismatch,
            TransferError::AccountNotActive => ErrorCode::AccountNotActive,
            TransferError::ParentNotFound => ErrorCode::ParentNotFound,
            TransferError::NotAFolder => ErrorCode::NotAFolder,
            TransferError::ParentDeleted => ErrorCode::ParentDeleted,
            TransferError::BlockHashMismatch => ErrorCode::BlockHashMismatch,
            TransferError::BlockLengthMismatch => ErrorCode::BlockLengthMismatch,
            TransferError::BlockConflict => ErrorCode::BlockConflict,
            TransferError::NeedBlocks(_) => ErrorCode::NeedBlocks,
            TransferError::EditedSince => ErrorCode::EditedSince,
            TransferError::ContentUnavailable => ErrorCode::ContentUnavailable,
            TransferError::RangeNotSatisfiable => ErrorCode::RangeNotSatisfiable,
            TransferError::UploadSessionGone => ErrorCode::UploadSessionGone,
            TransferError::Overloaded { .. } => ErrorCode::Overloaded,
        }
    }
}

impl From<TransferError> for BusinessError {
    fn from(error: TransferError) -> BusinessError {
        let code = error.code();
        let params = match error {
            TransferError::NeedBlocks(need_blocks) => BusinessParams {
                need_blocks,
                ..BusinessParams::default()
            },
            TransferError::Overloaded { retry_after } => BusinessParams {
                retry_after: Some(retry_after),
                ..BusinessParams::default()
            },
            _ => BusinessParams::default(),
        };
        BusinessError::new(code, params)
    }
}

impl From<TransferError> for DomainError {
    fn from(error: TransferError) -> DomainError {
        DomainError::Business(error.into())
    }
}
