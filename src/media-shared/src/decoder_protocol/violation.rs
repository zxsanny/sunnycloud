//! The one failure class of the decoder socket: the peer sent something the protocol forbids.

use std::fmt;

use crate::decoder_protocol::DecodeError;

/// Why a frame or request was refused. Carries no peer text, so it can be logged as is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// The announced payload length is above the 64 KiB cap.
    Oversize,
    NotUtf8,
    /// Not exactly one JSON value.
    Json,
    /// Valid JSON that does not match the schema: unknown field, unknown enumeration value,
    /// missing field, wrong type, or a number outside the type (for example a negative `u32`).
    Schema,
    /// A number or text of the named field is outside the range table.
    OutOfRange(&'static str),
    /// A request frame where a reply is expected, or the other way round.
    WrongDirection,
    /// A request that the session state does not allow.
    OutOfSequence,
}

/// Hostile-class outcome: the connection ends on both sides (contract, session rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolViolation(pub ViolationKind);

impl fmt::Display for ProtocolViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ViolationKind::Oversize => f.write_str("protocol violation: frame above 64 KiB"),
            ViolationKind::NotUtf8 => f.write_str("protocol violation: payload is not UTF-8"),
            ViolationKind::Json => f.write_str("protocol violation: payload is not one JSON value"),
            ViolationKind::Schema => {
                f.write_str("protocol violation: payload does not match the schema")
            }
            ViolationKind::OutOfRange(field) => {
                write!(f, "protocol violation: {field} is out of range")
            }
            ViolationKind::WrongDirection => {
                f.write_str("protocol violation: message in the wrong direction")
            }
            ViolationKind::OutOfSequence => {
                f.write_str("protocol violation: request out of sequence")
            }
        }
    }
}

impl std::error::Error for ProtocolViolation {}

impl From<ProtocolViolation> for DecodeError {
    fn from(_: ProtocolViolation) -> Self {
        DecodeError::ProtocolViolation
    }
}
