//! Failure taxonomy, stable wire codes with their single status table, the correlation id and the
//! client-side problem parser. No HTTP crate: statuses are plain `u16`.

mod business;
mod code;
mod correlation;
mod problem;

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "server")]
mod domain;

pub use business::{BusinessError, BusinessParams, FieldProblem, ResyncReason};
#[cfg(feature = "client")]
pub use client::{ApiError, TransportKind};
pub use code::ErrorCode;
pub use correlation::{CorrelationId, uuid_v7};
#[cfg(feature = "server")]
pub use domain::DomainError;
pub use problem::ProblemBody;
