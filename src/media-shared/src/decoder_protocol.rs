//! Decoder protocol v1 (10 section 3): the message types, the frame codec, the strict parser and
//! the session rules, shared by the runner (10) and the supervisor (09). Each side treats the
//! other as hostile, so there is one codec and no lenient path. Synchronous; no I/O of its own.
//!
//! # Decisions of contract `decoder_protocol` v1 (open points 1-6)
//!
//! 1. **Prefix byte order**: big-endian (network order) `u32`, then that many payload bytes.
//! 2. **Envelope**: the payload is one externally tagged [`Message`]: `{"request": R}`,
//!    `{"response": P}` or `{"event": E}`, where `R`, `P`, `E` are the serde forms of
//!    [`Request`], [`Response`], [`Event`] (snake_case; unit variants are bare strings, for
//!    example `{"request":"cancel"}`, `{"event":"alive"}`). The runner accepts only `request`,
//!    the supervisor only `response` and `event`; the wrong role is a violation.
//! 3. **Encodings**: every duration is an integer of milliseconds (`limits.wall`,
//!    `limits.stall_timeout`, `duration_ms`, `poster_at_ms`, `out_time_ms`);
//!    `build_timestamp` is an integer of Unix seconds. RFC 3339 is not accepted.
//! 4. **Ranges** (a number outside its serde type is a `Schema` violation, outside this table
//!    an `OutOfRange` violation; the table lives in [`ranges`]):
//!
//!    | Field | Range |
//!    |---|---|
//!    | `hello.protocol` | any `u32` (a wrong value is `ProtocolMismatch`, not a violation) |
//!    | `forbidden_targets` | 0..=64 entries of 1..=255 printable ASCII bytes without spaces |
//!    | `job_dir` | 1..=4096 bytes, no NUL |
//!    | `render_still.max_edge`, `plan.scale_to_short_edge` | 1..=16383 |
//!    | `poster_at_ms`, `duration_ms`, `out_time_ms` | 0..=604,800,000 (7 days) |
//!    | `limits.*` (all five) | 1..=type maximum; above the runner's ceiling is accepted and clamped |
//!    | `Capabilities` texts | 1..=128 printable ASCII bytes without spaces; `build_timestamp` 1..=4,102,444,800; `nvenc_sessions` 0..=64 |
//!    | `self_check` | 0..=64 items; `name` 1..=128 printable ASCII bytes without spaces |
//!    | `Probe.container`, codec, profile, `pix_fmt`, primaries, transfer, matrix, `audio.codec` | 1..=128 printable ASCII bytes (spaces allowed) |
//!    | `Probe.video.width`, `height` | 1..=1,048,576 |
//!    | `Probe.video.rotation` | -360..=360 |
//!    | `Probe.video.bitrate` | 0..=10^12 |
//!    | `ImageResult.width`, `height` | 1..=16383 |
//!    | `ImageResult.bytes`, `TranscodeResult.bytes` | 1..=2^50 |
//!    | `EncoderFailed.exit_code` | 1..=255 |
//!    | `Progress.speed` | finite, 0..=1000 |
//!
//!    A sender is held to the same table: [`write_message`] refuses what the parser would refuse.
//! 5. **Sequence** ([`Session`]): `hello` may repeat when idle; a job request (`probe`,
//!    `render_still`, `transcode`) while a job runs is `Busy`; `cancel` is always accepted (on an
//!    idle session it is a no-op, because it can lose a race with the job's own response);
//!    `hello` or `self_check` while a job runs is out of sequence and a violation. No `hello` is
//!    required before the first request.
//! 6. **Second connection**: queued, not refused. The runner does not `accept` the next
//!    connection until the current one is closed, so the parser has no rule for it.

use serde::{Deserialize, Serialize};

use crate::policy::{Plan, Probe};

mod frame;
mod message;
pub mod ranges;
mod session;
mod violation;

pub use frame::{
    FrameReader, MAX_PAYLOAD, PREFIX_LEN, Polled, ReadError, WriteError, write_message,
};
pub use message::{Message, Reply, encode_message, parse_message, parse_reply, parse_request};
pub use session::{Admission, Session};
pub use violation::{ProtocolViolation, ViolationKind};

/// Per-job limits. The runner clamps each value to its own ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cpu: u32,
    pub memory_bytes: u64,
    /// Wall-clock limit in milliseconds.
    pub wall: u64,
    /// Longest silence of an ffmpeg progress stream, in milliseconds.
    pub stall_timeout: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        protocol: u32,
    },
    SelfCheck {
        /// `host:port` targets the decoder must fail to reach.
        forbidden_targets: Vec<String>,
    },
    Probe {
        job_dir: String,
    },
    RenderStill {
        job_dir: String,
        max_edge: u32,
        poster_at_ms: Option<u64>,
        limits: Limits,
    },
    Transcode {
        job_dir: String,
        plan: Plan,
        limits: Limits,
    },
    Cancel,
}

/// Reply to `hello`. `build_timestamp` is Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub protocol: u32,
    pub vips: String,
    pub ffmpeg: String,
    pub libheif: String,
    pub build_timestamp: u64,
    pub nvenc_probe_ok: bool,
    pub nvenc_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfCheckItem {
    pub name: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckReport(pub Vec<SelfCheckItem>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageResult {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscodeResult {
    pub probe_of_output: Probe,
    pub bytes: u64,
}

/// Which limit a job exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    Wall,
    Memory,
    Pixels,
    Output,
}

/// Typed failures. Variants carry enumerations and numbers only, never child output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DecodeError {
    ProtocolMismatch,
    ProtocolViolation,
    Unsupported,
    Corrupt,
    LimitExceeded { limit: LimitKind },
    Busy,
    Stalled,
    EncoderFailed { exit_code: i32 },
    OutputInvalid,
}

/// One reply per request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Capabilities(Capabilities),
    SelfCheck(SelfCheckReport),
    Probe(Probe),
    ImageResult(ImageResult),
    TranscodeResult(TranscodeResult),
    Cancelled,
    Error(DecodeError),
}

/// Sent while a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Alive,
    Progress { out_time_ms: u64, speed: f64 },
}
