//! Transcode policy types: what the decoder probes, what the supervisor decides, what a job
//! must produce, and the `Params` values of each `params_version`. `is_compliant`, `decide` and
//! `poster_offset_ms` are behaviour of the media-jobs epic.
//!
//! `Probe` and `Plan` cross the decoder socket, so they reject unknown fields. `Plan` carries
//! enumerations, booleans and bounded numbers only: no string can become an ffmpeg argument.

use sc_ids::FileHash;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::decoder_protocol::Capabilities;
use crate::types::{DerivedKind, JobKind};

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

/// The `params_version` that new jobs are planned with.
pub const CURRENT_PARAMS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StillFormat {
    Webp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum H264Profile {
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    Aac,
}

/// The transcode output settings of one `params_version`. The size bounds apply whatever the
/// rotation metadata says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoParams {
    pub codec: VideoCodec,
    pub profile: H264Profile,
    pub max_short_side: u32,
    pub max_long_side: u32,
    pub audio: AudioCodec,
    pub faststart: bool,
    pub tone_map_to_sdr: bool,
}

/// The output settings of one `params_version`. Changing any value needs a new version; the
/// per-kind size caps are API settings and CRF and preset are not fixed yet (see
/// `worker_job_protocol.md`, open point 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    pub thumb_edge: u32,
    pub preview_edge: u32,
    pub still_format: StillFormat,
    pub apply_orientation: bool,
    pub video: VideoParams,
}

static PARAMS_V1: Params = Params {
    thumb_edge: 320,
    preview_edge: 1440,
    still_format: StillFormat::Webp,
    apply_orientation: true,
    video: VideoParams {
        codec: VideoCodec::H264,
        profile: H264Profile::High,
        max_short_side: 1080,
        max_long_side: 1920,
        audio: AudioCodec::Aac,
        faststart: true,
        tone_map_to_sdr: true,
    },
};

impl Params {
    /// `None` for a version this build does not know.
    pub const fn for_version(params_version: u32) -> Option<&'static Params> {
        match params_version {
            1 => Some(&PARAMS_V1),
            _ => None,
        }
    }
}

const STILL_TYPES: [&str; 2] = ["image/webp", "image/jpeg"];
const TRANSCODE_TYPES: [&str; 1] = ["video/mp4"];

/// What a job must produce for one derived kind. `max_edge` is the long edge of a still.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSpec {
    pub kind: DerivedKind,
    pub media_types: Vec<String>,
    pub max_edge: Option<u32>,
}

impl OutputSpec {
    pub fn for_kind(kind: DerivedKind, params: &Params) -> Self {
        let (types, max_edge): (&[&str], Option<u32>) = match kind {
            DerivedKind::Thumb => (&STILL_TYPES, Some(params.thumb_edge)),
            DerivedKind::Preview => (&STILL_TYPES, Some(params.preview_edge)),
            DerivedKind::Transcode => (&TRANSCODE_TYPES, None),
        };
        Self {
            kind,
            media_types: types.iter().map(|t| (*t).to_owned()).collect(),
            max_edge,
        }
    }

    /// Exact comparison against the type the API sniffed: parameters, case and spaces do not match.
    pub fn accepts(&self, media_type: &str) -> bool {
        self.media_types.iter().any(|t| t == media_type)
    }
}

/// One job to enqueue, as the planner returns it. Priority is chosen at enqueue time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSpec {
    pub account_id: Uuid,
    pub revision_id: Uuid,
    pub kind: JobKind,
    pub content_hash: FileHash,
    pub params_version: u32,
}
