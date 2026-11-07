use contracts::{
    AccountServiceError, AdminError, DeletionError, DerivedError, PublicShareError, ShareError,
    ThumbnailStateError, VaultError, WorkerAuthError, WorkerJobError,
};
use sc_error::{CorrelationId, DomainError, ErrorCode};

use super::{PortError, business, fault, invalid_field, no_wire_code, not_found, retry_after};

impl PortError for ShareError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::TargetNotFound | Self::NotFound => not_found(),
            Self::TargetNotShareable => business(ErrorCode::TargetNotShareable),
            Self::ExpiryOutOfRange => invalid_field("expires_in", "out_of_range"),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for PublicShareError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::NotFound => not_found(),
            Self::WrongPassword | Self::PasswordRequired => business(ErrorCode::PasswordRequired),
            Self::RateLimited { retry_after: delay } => retry_after(ErrorCode::RateLimited, delay),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for AdminError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::LastAdmin => business(ErrorCode::LastAdmin),
            Self::NotFound => not_found(),
            Self::InvalidLimit => invalid_field("limit", "out_of_range"),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for AccountServiceError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for DeletionError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::Forbidden => business(ErrorCode::Forbidden),
            Self::LastAdmin => business(ErrorCode::LastAdmin),
            Self::ConfirmationMismatch => invalid_field("confirmation", "mismatch"),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for ThumbnailStateError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for DerivedError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::NotReady => business(ErrorCode::ThumbnailNotReady),
            Self::NotFound => business(ErrorCode::ThumbnailUnavailable),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for WorkerAuthError {
    fn into_domain(self, _request_id: CorrelationId) -> DomainError {
        match self {
            Self::Forbidden => business(ErrorCode::Forbidden),
        }
    }
}

impl PortError for WorkerJobError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::LeaseLost => business(ErrorCode::LeaseLost),
            Self::RevisionObsolete => business(ErrorCode::RevisionObsolete),
            Self::OutputTooLarge => business(ErrorCode::OutputTooLarge),
            Self::OutputTypeRejected => business(ErrorCode::OutputTypeRejected),
            Self::OutputKindRejected => business(ErrorCode::OutputKindRejected),
            Self::Storage(source) | Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for VaultError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::VaultExists => no_wire_code("VaultError::VaultExists", request_id),
            Self::HeaderTooLarge => business(ErrorCode::SizeTooLarge),
            Self::NotFound => not_found(),
            Self::PreconditionFailed => no_wire_code("VaultError::PreconditionFailed", request_id),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}
