//! The per-port error conversion: each variant of every port error in [`crate::ApiServices`] to its `sc_error`
//! code, so a handler binds, calls and maps. Every `match` here is exhaustive: a new variant fails the build
//! until it has a code.

mod files;
mod identity;
mod services;

use std::time::Duration;

use contracts::Fault;
use sc_error::{
    BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode, FieldProblem,
};

pub use files::entry_error_code;

/// A port error that [`crate::ApiProblem`] can answer.
pub trait PortError {
    /// The failure for the request `request_id`. A tier-3 variant keeps its fault for the log only.
    fn into_domain(self, request_id: CorrelationId) -> DomainError;
}

/// A port variant whose meaning has no row in the `sc_error` table yet. It is answered as an opaque 500 and
/// logged with this name, so the gap is visible instead of being sent to a client under a wrong code.
#[derive(Debug, thiserror::Error)]
#[error("{0} has no sc_error code")]
struct NoWireCode(&'static str);

fn business(code: ErrorCode) -> DomainError {
    DomainError::Business(BusinessError::new(code, BusinessParams::default()))
}

fn not_found() -> DomainError {
    DomainError::Business(BusinessError::not_found())
}

fn retry_after(code: ErrorCode, delay: Duration) -> DomainError {
    DomainError::Business(BusinessError::new(
        code,
        BusinessParams {
            retry_after: Some(delay),
            ..BusinessParams::default()
        },
    ))
}

fn invalid_field(field: &str, problem: &str) -> DomainError {
    DomainError::Validation(vec![FieldProblem {
        field: field.to_owned(),
        problem: problem.to_owned(),
    }])
}

fn fault(source: Fault, request_id: CorrelationId) -> DomainError {
    DomainError::unexpected(source, request_id)
}

fn no_wire_code(variant: &'static str, request_id: CorrelationId) -> DomainError {
    DomainError::unexpected(NoWireCode(variant), request_id)
}
