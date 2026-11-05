//! Decoder protocol v1 message types (10 section 3). The decoder is untrusted, so every type
//! rejects unknown fields. Framing, the 64 KiB frame cap and numeric range checks are not here.

use serde::{Deserialize, Serialize};

use crate::policy::{Plan, Probe};

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
