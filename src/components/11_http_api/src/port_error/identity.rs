use contracts::{
    AuthenticatorError, DeviceGrantError, InviteError, RevalidatorError, SessionError, SignInError,
};
use sc_error::{CorrelationId, DomainError, ErrorCode};

use super::{PortError, business, fault, invalid_field, no_wire_code, not_found, retry_after};

impl PortError for AuthenticatorError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::Unauthenticated => business(ErrorCode::Unauthenticated),
            Self::Forbidden => business(ErrorCode::Forbidden),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for RevalidatorError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for SessionError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::ReuseDetected | Self::Unauthenticated => business(ErrorCode::Unauthenticated),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for SignInError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::UnknownProvider => invalid_field("provider", "unknown"),
            Self::InvalidReturnPath => invalid_field("return_to", "invalid"),
            Self::HostMismatch => invalid_field("host", "mismatch"),
            Self::UntrustedState => business(ErrorCode::InvalidRequest),
            Self::RateLimited { retry_after: delay } => retry_after(ErrorCode::RateLimited, delay),
            Self::TrustedFailure { code } => business(code),
            Self::ProviderUnavailable => business(ErrorCode::ServiceUnavailable),
            Self::InvalidCode => no_wire_code("SignInError::InvalidCode", request_id),
            Self::IdentityTaken => no_wire_code("SignInError::IdentityTaken", request_id),
            Self::InvitationRequired => no_wire_code("SignInError::InvitationRequired", request_id),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for DeviceGrantError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::AuthorizationPending => {
                no_wire_code("DeviceGrantError::AuthorizationPending", request_id)
            }
            Self::SlowDown => no_wire_code("DeviceGrantError::SlowDown", request_id),
            Self::AccessDenied => no_wire_code("DeviceGrantError::AccessDenied", request_id),
            Self::ExpiredToken => no_wire_code("DeviceGrantError::ExpiredToken", request_id),
            Self::UnknownCode | Self::Expired | Self::AlreadyDecided | Self::NotFound => {
                not_found()
            }
            Self::RateLimited { retry_after: delay } => retry_after(ErrorCode::RateLimited, delay),
            Self::Forbidden => business(ErrorCode::Forbidden),
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}

impl PortError for InviteError {
    fn into_domain(self, request_id: CorrelationId) -> DomainError {
        match self {
            Self::Unexpected(source) => fault(source, request_id),
        }
    }
}
