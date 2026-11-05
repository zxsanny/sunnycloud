//! Tier 1 and tier 2 values: field problems and expected business failures.

use crate::code::ErrorCode;
use serde::Serialize;
use std::time::Duration;
use thiserror::Error;

/// Why the changes feed asks the client to resync (`resync_required`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    EpochChanged,
    BelowPurgeHorizon,
    CursorAhead,
}

impl ResyncReason {
    /// Maps a wire string; `None` for a reason this build does not know.
    pub fn from_wire(wire: &str) -> Option<ResyncReason> {
        match wire {
            "epoch_changed" => Some(Self::EpochChanged),
            "below_purge_horizon" => Some(Self::BelowPurgeHorizon),
            "cursor_ahead" => Some(Self::CursorAhead),
            _ => None,
        }
    }
}

/// Typed parameters a business failure may carry. Empty `need_blocks` means absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BusinessParams {
    pub need_blocks: Vec<u32>,
    /// Travels on the wire as whole seconds, rounded up.
    pub retry_after: Option<Duration>,
    pub reason: Option<ResyncReason>,
    pub min_version: Option<String>,
}

/// One rejected input field. `problem` names the rule broken and never echoes the input value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldProblem {
    pub field: String,
    pub problem: String,
}

/// Tier 2: a defined outcome the caller can recover from. No backtrace, no allocation beyond its parameters.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{}", .code.as_str())]
pub struct BusinessError {
    pub code: ErrorCode,
    pub params: BusinessParams,
}

impl BusinessError {
    pub fn new(code: ErrorCode, params: BusinessParams) -> BusinessError {
        BusinessError { code, params }
    }

    /// The one constructor for missing ids, cross-account ids and every missing, expired or
    /// revoked share, so their response bytes cannot differ.
    pub fn not_found() -> BusinessError {
        BusinessError::new(ErrorCode::NotFound, BusinessParams::default())
    }
}
