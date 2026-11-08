//! How every error of a job step becomes the call that ends the job (09 section 3, steps 3, 5 and 6).
//! The disposition of each case is the one the documents name; the cases they leave open are marked.

use media_shared::decoder_protocol::DecodeError;
use media_shared::types::{Disposition, FailureReport};

use super::lease::LeaseEnd;
use crate::api::ApiError;
use crate::decoder_client::{CallError, Unhealthy};
use crate::policy_step::{OutputInvalid, StepError};
use crate::scratch::ScratchError;

pub(super) const INPUT_TOO_LARGE: &str = "input_too_large";
pub(super) const OUTPUT_REJECTED: &str = "output_rejected";
const SCRATCH_IO: &str = "ScratchIo";

/// Why a job did not reach `derived` or `skipped_compliant` on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stop {
    /// 404 on input or output: close with `complete(obsolete)`.
    Obsolete,
    /// 409: another worker holds the job. Nothing more is sent.
    LeaseLost,
    /// 403: the worker is not ready and stops claiming. Nothing more is sent.
    Refused,
    /// The shutdown grace passed: the lease is released so another claim can take the job.
    Released,
    /// The decoder broke the protocol, went silent or ran past the wall limit: the slot leaves service and the
    /// job is failed `Retry`.
    Unhealthy(Unhealthy),
    Fail(FailureReport),
}

impl Stop {
    pub(super) fn retry(code: &str) -> Self {
        Self::fail(code, Disposition::Retry)
    }

    pub(super) fn permanent(code: &str) -> Self {
        Self::fail(code, Disposition::Permanent)
    }

    fn fail(code: &str, disposition: Disposition) -> Self {
        Stop::Fail(FailureReport {
            code: code.to_owned(),
            disposition,
        })
    }

    /// The job scratch path is not text, so no `job_dir` can be sent to the decoder.
    pub(super) fn scratch_io() -> Self {
        Self::retry(SCRATCH_IO)
    }
}

impl From<LeaseEnd> for Stop {
    fn from(end: LeaseEnd) -> Self {
        match end {
            LeaseEnd::Lost => Stop::LeaseLost,
            LeaseEnd::Refused => Stop::Refused,
            LeaseEnd::Aborted => Stop::Released,
        }
    }
}

impl From<&ApiError> for Stop {
    fn from(error: &ApiError) -> Self {
        match error {
            ApiError::Forbidden => Stop::Refused,
            ApiError::LeaseLost => Stop::LeaseLost,
            ApiError::Gone => Stop::Obsolete,
            // 09 section 6: a larger input is `TooLarge`, permanent. A 413 or 422 on an output is the same
            // output again on a retry; the documents do not name a disposition for them.
            ApiError::TooLarge => Stop::permanent("TooLarge"),
            ApiError::TypeRejected => Stop::permanent("TypeRejected"),
            ApiError::HashMismatch => Stop::retry("HashMismatch"),
            ApiError::Transient(_) => Stop::retry("Transient"),
            // An answer the contract does not describe has no documented disposition.
            ApiError::Protocol(_) => Stop::retry("Protocol"),
            ApiError::Destination(_) => Stop::scratch_io(),
        }
    }
}

impl From<&ScratchError> for Stop {
    fn from(error: &ScratchError) -> Self {
        match error {
            ScratchError::InputTooLarge { .. } => Stop::permanent(INPUT_TOO_LARGE),
            ScratchError::OutputRejected(_) => Stop::permanent(OUTPUT_REJECTED),
            ScratchError::Io { .. } => Stop::scratch_io(),
        }
    }
}

impl From<StepError> for Stop {
    /// Neither case has a documented disposition. A poster probe without a duration gives the same answer
    /// again, so it is permanent; a `params_version` this build does not know may be known to a newer worker,
    /// so it is retried.
    fn from(error: StepError) -> Self {
        match error {
            StepError::UnknownParamsVersion(_) => Stop::retry("UnknownParamsVersion"),
            StepError::MissingDuration => Stop::permanent("MissingDuration"),
        }
    }
}

impl From<OutputInvalid> for Stop {
    fn from(error: OutputInvalid) -> Self {
        Stop::Fail(error.failure_report())
    }
}

impl Unhealthy {
    /// The failure the API is sent. All three are transient: another attempt, possibly on another slot, can
    /// succeed.
    pub(super) fn report(self) -> FailureReport {
        let code = match self {
            Unhealthy::Silent => "DecoderSilent",
            Unhealthy::WallLimit => "WallLimit",
            Unhealthy::Violation => "ProtocolViolation",
        };
        FailureReport {
            code: code.to_owned(),
            disposition: Disposition::Retry,
        }
    }
}

/// `Stalled` and `EncoderFailed` get one rerun inside the claim (09 section 3, step 5).
pub(super) fn rerunnable(error: &CallError) -> bool {
    matches!(
        error,
        CallError::Decode(DecodeError::Stalled | DecodeError::EncoderFailed { .. })
    )
}

/// The end of a decoder call that gets no rerun, or whose rerun failed too. The second `Stalled` is permanent
/// by the documents; a second `EncoderFailed` is not named there and is treated the same way.
pub(super) fn decoder_stop(error: &CallError) -> Stop {
    match error {
        CallError::Decode(decode) => decode_stop(*decode),
        CallError::Closed | CallError::Io(_) | CallError::Disconnected => {
            Stop::retry("DecoderUnreachable")
        }
        CallError::Violation(_) => Stop::Unhealthy(Unhealthy::Violation),
        CallError::InvalidRequest(_) => Stop::retry("InvalidRequest"),
        CallError::Cancelled => Stop::retry("Cancelled"),
    }
}

fn decode_stop(error: DecodeError) -> Stop {
    match error {
        DecodeError::Unsupported => Stop::permanent("Unsupported"),
        DecodeError::Corrupt => Stop::permanent("Corrupt"),
        DecodeError::LimitExceeded { .. } => Stop::permanent("LimitExceeded"),
        DecodeError::OutputInvalid => Stop::from(OutputInvalid),
        DecodeError::Stalled => Stop::permanent("Stalled"),
        DecodeError::EncoderFailed { .. } => Stop::permanent("EncoderFailed"),
        DecodeError::ProtocolViolation | DecodeError::ProtocolMismatch => {
            Stop::Unhealthy(Unhealthy::Violation)
        }
        DecodeError::Busy => Stop::retry("Busy"),
    }
}
