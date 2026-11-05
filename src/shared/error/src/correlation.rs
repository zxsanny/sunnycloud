//! The correlation id: `CorrelationId` in Rust, `request_id` on the wire.

use serde::Serialize;
use std::fmt;
use uuid::Uuid;

/// Id of one request, shared by the `X-Request-Id` header, the problem body and the logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CorrelationId(Uuid);

impl CorrelationId {
    pub const fn new(id: Uuid) -> CorrelationId {
        CorrelationId(id)
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Parses a header value; only the 36-character hyphenated UUID form is accepted so the echoed id stays canonical.
    pub fn parse(value: &str) -> Option<CorrelationId> {
        let value = value.trim();
        if value.len() != 36 {
            return None;
        }
        match Uuid::parse_str(value) {
            Ok(id) => Some(CorrelationId(id)),
            Err(_) => None,
        }
    }

    /// Keeps a well-formed incoming id; otherwise takes the caller's fresh UUIDv7 (see [`uuid_v7`]).
    pub fn from_request(header: Option<&str>, fresh: impl FnOnce() -> Uuid) -> CorrelationId {
        header
            .and_then(CorrelationId::parse)
            .unwrap_or_else(|| CorrelationId(fresh()))
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

/// Builds a UUIDv7 from a Unix millisecond timestamp and 80 caller-supplied random bits.
/// The `uuid` crate's v7 feature needs an RNG that wasm32 builds do not have, so the layout is set by hand.
pub fn uuid_v7(unix_ms: u64, random: [u8; 10]) -> Uuid {
    let timestamp = u128::from(unix_ms) & 0xFFFF_FFFF_FFFF;
    let rand_a = (u128::from(random[0]) << 8 | u128::from(random[1])) & 0x0FFF;
    let rand_b = random[2..]
        .iter()
        .fold(0u128, |acc, byte| acc << 8 | u128::from(*byte))
        & ((1u128 << 62) - 1);
    Uuid::from_u128(timestamp << 80 | 0x7 << 76 | rand_a << 64 | 0b10 << 62 | rand_b)
}
