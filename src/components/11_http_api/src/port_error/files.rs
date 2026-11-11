use contracts::{
    ChangeFeedError, CommitServiceError, DownloadError, EntryError, ResyncReason, TreeReaderError,
    TreeServiceError, UploadError,
};
use sc_error::{
    BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode,
    ResyncReason as WireReason,
};

use super::{PortError, business, fault, invalid_field, no_wire_code, not_found, retry_after};

impl PortError for TreeServiceError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::NameInvalid(_) => business(ErrorCode::InvalidName),
            Self::NameConflict => business(ErrorCode::NameConflict),
            Self::ParentDeleted => business(ErrorCode::ParentDeleted),
            Self::NotFound => not_found(),
            Self::NotAFolder => business(ErrorCode::NotAFolder),
            Self::Cycle => business(ErrorCode::Cycle),
            Self::IsRoot => business(ErrorCode::InvalidRequest),
            Self::EditedSince { .. } => business(ErrorCode::EditedSince),
            Self::InvalidCursor => business(ErrorCode::InvalidCursor),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for TreeReaderError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::NotFound => not_found(),
            Self::NotAFolder => business(ErrorCode::NotAFolder),
            Self::InvalidCursor => business(ErrorCode::InvalidCursor),
            Self::BatchTooLarge => business(ErrorCode::BatchTooLarge),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for UploadError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::InvalidName => business(ErrorCode::InvalidName),
            Self::ParentNotFound => business(ErrorCode::ParentNotFound),
            Self::NotAFolder => business(ErrorCode::NotAFolder),
            Self::SizeTooLarge => business(ErrorCode::SizeTooLarge),
            Self::QuotaExceeded => business(ErrorCode::QuotaExceeded),
            Self::SessionCapReached => business(ErrorCode::SessionCapExceeded),
            Self::StagedCapReached => business(ErrorCode::StagedCapReached),
            Self::FreeSpaceFloor | Self::StorageFull => business(ErrorCode::FreeSpaceFloor),
            Self::ClientKeyMismatch => no_wire_code("UploadError::ClientKeyMismatch", request_id),
            Self::AccountNotActive => business(ErrorCode::AccountNotActive),
            Self::UploadNotFound => business(ErrorCode::UploadSessionGone),
            Self::IndexOutOfRange => invalid_field("index", "out_of_range"),
            Self::LengthMismatch => business(ErrorCode::BlockLengthMismatch),
            Self::HashMismatch => business(ErrorCode::BlockHashMismatch),
            Self::BlockConflict => business(ErrorCode::BlockConflict),
            Self::Overloaded => business(ErrorCode::Overloaded),
            Self::Storage(source) | Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for CommitServiceError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::NeedBlocks(indexes) => DomainError::Business(BusinessError::new(
                ErrorCode::NeedBlocks,
                BusinessParams {
                    need_blocks: indexes,
                    ..BusinessParams::default()
                },
            )),
            Self::BlocklistMismatch => business(ErrorCode::BlocklistMismatch),
            Self::UploadNotFound => business(ErrorCode::UploadSessionGone),
            Self::InvalidName => business(ErrorCode::InvalidName),
            Self::EditedSince => business(ErrorCode::EditedSince),
            Self::ParentGone => business(ErrorCode::ParentDeleted),
            Self::ParentNotFound => business(ErrorCode::ParentNotFound),
            Self::NotAFolder => business(ErrorCode::NotAFolder),
            Self::AccountNotActive => business(ErrorCode::AccountNotActive),
            Self::BatchTooLarge => business(ErrorCode::BatchTooLarge),
            Self::CommitIdReused => business(ErrorCode::CommitIdReused),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

/// The code of one failed entry of a batch commit, for the per-entry result in the 200 body.
pub fn entry_error_code(error: &EntryError) -> ErrorCode {
    match error {
        EntryError::InvalidName => ErrorCode::InvalidName,
        EntryError::EditedSince => ErrorCode::EditedSince,
        EntryError::ParentGone => ErrorCode::ParentDeleted,
        EntryError::ParentNotFound => ErrorCode::ParentNotFound,
        EntryError::NotAFolder => ErrorCode::NotAFolder,
        EntryError::BlocklistMismatch => ErrorCode::BlocklistMismatch,
        EntryError::UploadNotFound => ErrorCode::UploadSessionGone,
        EntryError::EffectFailed(code) => *code,
    }
}

impl PortError for DownloadError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::NotFound => not_found(),
            Self::RangeNotSatisfiable { .. } => business(ErrorCode::RangeNotSatisfiable),
            Self::ContentUnavailable => business(ErrorCode::ContentUnavailable),
            Self::Storage(source) | Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for ChangeFeedError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::ResyncRequired { reason } => DomainError::Business(BusinessError::new(
                ErrorCode::ResyncRequired,
                BusinessParams {
                    reason: Some(match reason {
                        ResyncReason::EpochChanged => WireReason::EpochChanged,
                        ResyncReason::BelowPurgeHorizon => WireReason::BelowPurgeHorizon,
                        ResyncReason::CursorAhead => WireReason::CursorAhead,
                    }),
                    ..BusinessParams::default()
                },
            )),
            Self::Unauthenticated => business(ErrorCode::Unauthenticated),
            Self::BadCursor => business(ErrorCode::InvalidCursor),
            Self::TooManyHolds { retry_after: delay } => retry_after(ErrorCode::RateLimited, delay),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}
