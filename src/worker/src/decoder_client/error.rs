use std::io;

use media_shared::decoder_protocol::{DecodeError, ProtocolViolation};
use thiserror::Error;

/// Why one call to the decoder did not produce its result. No variant carries decoder text: `DecodeError`
/// holds enumerations and numbers only, and a violation names a class of fault.
#[derive(Debug, Error)]
pub enum CallError {
    /// The decoder answered with a typed failure; the connection stays usable except after
    /// `DecodeError::ProtocolViolation`, which ends it.
    #[error("decoder answered {0:?}")]
    Decode(DecodeError),
    /// The job ended through the caller's cancel.
    #[error("job cancelled")]
    Cancelled,
    /// The decoder sent something the protocol forbids; the connection was dropped.
    #[error(transparent)]
    Violation(#[from] ProtocolViolation),
    /// The request itself is out of range. Nothing was sent and the connection is untouched.
    #[error("request refused before sending: {0}")]
    InvalidRequest(ProtocolViolation),
    /// The decoder closed the stream (a runner exit); the connection was dropped.
    #[error("decoder closed the connection")]
    Closed,
    #[error("decoder socket failed: {0}")]
    Io(#[from] io::Error),
    /// An earlier call ended the connection or was abandoned before it finished.
    #[error("no live connection")]
    Disconnected,
}

/// Why a slot's gate did not pass. The detail is a fixed text: nothing the decoder said reaches `/readyz`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GateFailure {
    #[error("hello was not answered with protocol 1 capabilities")]
    Hello,
    #[error("self_check was not answered")]
    SelfCheck,
    /// `failed` items reported `ok = false`; an empty report counts as one failure, because it proves nothing.
    #[error("self_check reported failed items")]
    Items { failed: u32 },
    #[error("the decoder did not answer the handshake in time")]
    Silent,
}

impl GateFailure {
    pub(crate) fn detail(self) -> &'static str {
        match self {
            GateFailure::Hello => "hello failed",
            GateFailure::SelfCheck => "self_check failed",
            GateFailure::Items { .. } => "self_check reported failed items",
            GateFailure::Silent => "handshake timed out",
        }
    }
}

/// Reconnect backoff bounds that are not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("reconnect backoff needs 0 < initial <= max")]
pub struct BackoffError;

/// Why a slot that had passed the gate was taken out of use mid-job (09 section 3, step 4). The slot takes no
/// claim until it has reconnected and passed the gate again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Unhealthy {
    /// No frame of any kind for `decoder_silence_timeout`: the runner is hung.
    #[error("the decoder sent nothing for the silence timeout")]
    Silent,
    /// The decoder was still working when the wall limit of the job kind ran out.
    #[error("the decoder ran past the wall limit")]
    WallLimit,
    /// The decoder sent something the protocol forbids.
    #[error("the decoder broke the protocol")]
    Violation,
}

impl Unhealthy {
    pub(crate) fn detail(self) -> &'static str {
        match self {
            Unhealthy::Silent => "unhealthy: decoder silent",
            Unhealthy::WallLimit => "unhealthy: wall limit passed",
            Unhealthy::Violation => "unhealthy: protocol violation",
        }
    }
}
