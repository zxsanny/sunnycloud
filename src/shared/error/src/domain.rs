//! The three-tier failure sum returned by business code (feature `server`).

use crate::business::{BusinessError, FieldProblem};
use crate::code::ErrorCode;
use crate::correlation::CorrelationId;
use crate::problem::ProblemBody;
use std::error::Error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    /// Tier 1: bad caller input, returned as a value.
    #[error("validation failed: {} field problem(s)", .0.len())]
    Validation(Vec<FieldProblem>),
    /// Tier 2: a defined business outcome.
    #[error(transparent)]
    Business(#[from] BusinessError),
    /// Tier 3: a bug or infrastructure fault. `source` is for the log only and never reaches a body.
    #[error("unexpected failure (request_id {correlation_id})")]
    Unexpected {
        #[source]
        source: Box<dyn Error + Send + Sync>,
        correlation_id: CorrelationId,
    },
}

impl DomainError {
    pub fn unexpected(
        source: impl Into<Box<dyn Error + Send + Sync>>,
        correlation_id: CorrelationId,
    ) -> DomainError {
        DomainError::Unexpected {
            source: source.into(),
            correlation_id,
        }
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            DomainError::Validation(_) => ErrorCode::InvalidRequest,
            DomainError::Business(error) => error.code,
            DomainError::Unexpected { .. } => ErrorCode::Internal,
        }
    }

    /// The body for this failure under `request_id`. Tier 3 yields `internal` with nothing else.
    pub fn problem(&self, request_id: CorrelationId) -> ProblemBody {
        match self {
            DomainError::Validation(problems) => ProblemBody {
                problems: problems.clone(),
                ..ProblemBody::plain(ErrorCode::InvalidRequest, request_id)
            },
            DomainError::Business(error) => ProblemBody {
                params: error.params.clone(),
                ..ProblemBody::plain(error.code, request_id)
            },
            DomainError::Unexpected { .. } => ProblemBody::plain(ErrorCode::Internal, request_id),
        }
    }
}
