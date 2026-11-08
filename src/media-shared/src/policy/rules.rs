//! The pure transcode and media rules of `media_policy.md`. No I/O, no clock, no randomness.
//! Changing any number here bumps `params_version`.

use super::{
    AudioPlan, AudioStream, Decision, DecoderCapabilities, Encoder, EncoderPref, MediaKind,
    PARAMS_V1, Plan, Probe, VideoStream,
};
use crate::types::JobKind;

/// Video bitrate bound in bit/s, decimal and inclusive.
const MAX_VIDEO_BITRATE_BPS: u64 = 10_000_000;
const POSTER_DEFAULT_MS: u64 = 1_000;
/// Clips shorter than this take the poster at a tenth of their duration.
const POSTER_SHORT_CLIP_MS: u64 = 10_000;
const POSTER_SHORT_CLIP_DIVISOR: u64 = 10;

const CONTAINER_MP4: &str = "mp4";
const CODEC_H264: &str = "h264";
const CODEC_AAC: &str = "aac";
const PROFILES: [&str; 3] = ["baseline", "main", "high"];
const BT709: &str = "bt709";
const HDR_TRANSFERS: [&str; 2] = ["smpte2084", "arib-std-b67"];

const STILL_MEDIA_TYPES: [&str; 5] = [
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/gif",
    "image/heic",
];
const VIDEO_MEDIA_TYPES: [&str; 3] = ["video/mp4", "video/quicktime", "video/x-matroska"];

/// True when the clip needs neither a remux nor a transcode: every axis holds and the index is
/// first. A probe without a video stream, or with an unreported axis, is not compliant.
pub fn is_compliant(probe: &Probe) -> bool {
    index_free_axes_hold(probe) && probe.moov_first
}

/// `SkipCompliant` when every axis holds, `Remux` when only the index is at the end, otherwise
/// `Transcode`. The decoder capabilities are not read by any v1 rule; `pref` changes only the
/// encoder of a transcode plan.
pub fn decide(probe: &Probe, _capabilities: &DecoderCapabilities, pref: EncoderPref) -> Decision {
    if is_compliant(probe) {
        Decision::SkipCompliant
    } else if index_free_axes_hold(probe) {
        Decision::Remux
    } else {
        Decision::Transcode(transcode_plan(probe, pref))
    }
}

/// The CPU transcode plan for a clip: the plan a failed `Remux` or a failed hardware encode is rerun with. The
/// same rules as the `Transcode` of [`decide`] with the encoder fixed to the CPU.
pub fn cpu_transcode_plan(probe: &Probe) -> Plan {
    transcode_plan(probe, Encoder::Cpu)
}

/// Milliseconds into the clip where the poster frame is taken: 1,000 ms, or a tenth of the
/// duration for clips shorter than 10 s (rounded down), so never beyond the duration.
pub const fn poster_offset_ms(duration_ms: u64) -> u64 {
    if duration_ms < POSTER_SHORT_CLIP_MS {
        duration_ms / POSTER_SHORT_CLIP_DIVISOR
    } else {
        POSTER_DEFAULT_MS
    }
}

/// The jobs a media type gets. The match is exact (lowercase, no parameters), like
/// `OutputSpec::accepts`.
pub fn supported_kinds_for(media_type: &str) -> Vec<JobKind> {
    if STILL_MEDIA_TYPES.contains(&media_type) {
        vec![JobKind::Stills]
    } else if VIDEO_MEDIA_TYPES.contains(&media_type) {
        vec![JobKind::Poster, JobKind::Transcode]
    } else {
        Vec::new()
    }
}

/// Every compliance axis except the position of the index.
fn index_free_axes_hold(probe: &Probe) -> bool {
    let Some(video) = probe.video.as_ref() else {
        return false;
    };
    probe.kind == MediaKind::Video
        && is_mp4(&probe.container)
        && video_holds(video)
        && audio_holds(probe.audio.as_ref())
}

/// `container` is the demuxer name list ffprobe reports (`mov,mp4,m4a,3gp,3g2,mj2` for the MP4
/// family) or the plain name `mp4`; either way the `mp4` token must be present.
fn is_mp4(container: &str) -> bool {
    container
        .split(',')
        .any(|name| name.trim().eq_ignore_ascii_case(CONTAINER_MP4))
}

fn video_holds(video: &VideoStream) -> bool {
    is_named(video.codec.as_deref(), CODEC_H264)
        && PROFILES
            .iter()
            .any(|p| is_named(video.profile.as_deref(), p))
        && video
            .bitrate
            .is_some_and(|bps| bps <= MAX_VIDEO_BITRATE_BPS)
        && size_holds(video.width, video.height)
        && is_named(video.primaries.as_deref(), BT709)
        && is_named(video.transfer.as_deref(), BT709)
        && is_named(video.matrix.as_deref(), BT709)
}

/// The rotation metadata is not an input: the sides are ordered before the comparison.
fn size_holds(width: u32, height: u32) -> bool {
    let video = &PARAMS_V1.video;
    width.max(height) <= video.max_long_side && width.min(height) <= video.max_short_side
}

fn audio_holds(audio: Option<&AudioStream>) -> bool {
    audio.is_none_or(|a| a.codec.eq_ignore_ascii_case(CODEC_AAC))
}

fn is_named(value: Option<&str>, expected: &str) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case(expected))
}

fn is_hdr(video: Option<&VideoStream>) -> bool {
    video.is_some_and(|v| {
        HDR_TRANSFERS
            .iter()
            .any(|t| is_named(v.transfer.as_deref(), t))
    })
}

fn transcode_plan(probe: &Probe, encoder: Encoder) -> Plan {
    let video = &PARAMS_V1.video;
    Plan::Transcode {
        encoder,
        scale_to_short_edge: u16::try_from(video.max_short_side).unwrap_or(u16::MAX),
        tone_map: video.tone_map_to_sdr && is_hdr(probe.video.as_ref()),
        audio: if probe.audio.is_some() {
            AudioPlan::Aac
        } else {
            AudioPlan::None
        },
        faststart: video.faststart,
    }
}
