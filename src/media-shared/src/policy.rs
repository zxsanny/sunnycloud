//! Transcode policy types: what the decoder probes, what the supervisor decides, what a job
//! must produce. Types only; `is_compliant`, `decide`, `poster_offset_ms` and the `Params`
//! values are behaviour of the media-jobs epic.
//!
//! `Probe` and `Plan` cross the decoder socket, so they reject unknown fields. `Plan` carries
//! enumerations, booleans and bounded numbers only: no string can become an ffmpeg argument.

use serde::{Deserialize, Serialize};

use crate::decoder_protocol::Capabilities;
use crate::types::DerivedKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Photo,
    Video,
}

/// Facts the decoder reports about one input. Photos fill `video.width` and `video.height` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub kind: MediaKind,
    pub container: String,
    pub moov_first: bool,
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoStream {
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Rotation metadata in degrees.
    pub rotation: Option<i32>,
    /// Bits per second.
    pub bitrate: Option<u64>,
    pub pix_fmt: Option<String>,
    pub primaries: Option<String>,
    pub transfer: Option<String>,
    pub matrix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioStream {
    pub codec: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoder {
    Cpu,
    Nvenc,
}

/// The worker-local encoder choice; `Nvenc` only from v1.1.
pub type EncoderPref = Encoder;

/// What the decoder reads for the audio of a transcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioPlan {
    Aac,
    None,
}

/// The only job description the decoder accepts for a video: a lossless remux, or a transcode
/// described by an enum-only record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Plan {
    Remux,
    Transcode {
        encoder: Encoder,
        scale_to_short_edge: u16,
        tone_map: bool,
        audio: AudioPlan,
        faststart: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    SkipCompliant,
    Remux,
    Transcode(Plan),
}

/// The `hello` reply as the policy reads it.
pub type DecoderCapabilities = Capabilities;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum X264Preset {
    Ultrafast,
    Superfast,
    Veryfast,
    Faster,
    Fast,
    Medium,
    Slow,
    Slower,
    Veryslow,
    Placebo,
}

/// The output settings of one `params_version`; the values are the media-jobs epic's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    pub thumb_edge: u32,
    pub preview_edge: u32,
    pub webp_quality: u8,
    pub crf: u8,
    pub preset: X264Preset,
}

/// What a job must produce for one derived kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSpec {
    pub kind: DerivedKind,
    pub media_types: Vec<String>,
    pub max_edge: Option<u32>,
    pub max_bytes: u64,
}
