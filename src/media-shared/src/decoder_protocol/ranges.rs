//! The per-field range table of decoder protocol v1. A serde type already refuses negative
//! numbers, numbers above its width and unknown fields; this module adds the bounds the types
//! cannot express. Limits above the runner's ceilings are valid here: the runner clamps them.

use crate::decoder_protocol::{
    Capabilities, DecodeError, Event, ImageResult, Limits, Request, Response, SelfCheckReport,
};
use crate::decoder_protocol::{ProtocolViolation, ViolationKind};
use crate::policy::{AudioStream, Plan, Probe, VideoStream};

/// Longest edge of a WebP file (16383), so also the largest `max_edge`, `scale_to_short_edge`
/// and reply edge of a rendered still.
pub const MAX_EDGE: u32 = 16_383;
/// Largest width or height a `Probe` may report (a panorama within `photo_max_pixels`).
pub const MAX_DIMENSION: u32 = 1 << 20;
/// Seven days; bounds `duration_ms`, `poster_at_ms` and `out_time_ms`.
pub const MAX_DURATION_MS: u64 = 7 * 24 * 3600 * 1000;
/// 1 PiB; bounds every byte count of a reply.
pub const MAX_BYTES: u64 = 1 << 50;
/// 1 Tbit/s.
pub const MAX_BITRATE: u64 = 1_000_000_000_000;
pub const MAX_ROTATION_DEGREES: i32 = 360;
pub const MAX_JOB_DIR_BYTES: usize = 4096;
pub const MAX_FORBIDDEN_TARGETS: usize = 64;
pub const MAX_TARGET_BYTES: usize = 255;
pub const MAX_SELF_CHECK_ITEMS: usize = 64;
/// Longest text of a reply field (container, codec, version, check name).
pub const MAX_TEXT_BYTES: usize = 128;
pub const MAX_NVENC_SESSIONS: u32 = 64;
/// 2100-01-01T00:00:00Z in Unix seconds.
pub const MAX_BUILD_TIMESTAMP: u64 = 4_102_444_800;
pub const MAX_PROGRESS_SPEED: f64 = 1000.0;
pub const MAX_EXIT_CODE: i32 = 255;

type Check = Result<(), ProtocolViolation>;

fn out_of_range(field: &'static str) -> ProtocolViolation {
    ProtocolViolation(ViolationKind::OutOfRange(field))
}

fn within<T: PartialOrd>(field: &'static str, value: T, min: T, max: T) -> Check {
    // Written as a positive range test so that NaN is refused.
    if !(min <= value && value <= max) {
        return Err(out_of_range(field));
    }
    Ok(())
}

/// Printable ASCII, optionally with spaces, 1..=`max` bytes.
fn text(field: &'static str, value: &str, max: usize, allow_space: bool) -> Check {
    let allowed = |c: char| c.is_ascii_graphic() || (allow_space && c == ' ');
    if value.is_empty() || value.len() > max || !value.chars().all(allowed) {
        return Err(out_of_range(field));
    }
    Ok(())
}

fn optional_text(field: &'static str, value: Option<&String>) -> Check {
    value.map_or(Ok(()), |v| text(field, v, MAX_TEXT_BYTES, true))
}

fn job_dir(value: &str) -> Check {
    if value.is_empty() || value.len() > MAX_JOB_DIR_BYTES || value.contains('\0') {
        return Err(out_of_range("job_dir"));
    }
    Ok(())
}

fn limits(value: &Limits) -> Check {
    let nonzero = |field, v: u64| within(field, v, 1, u64::MAX);
    nonzero("limits.cpu", u64::from(value.cpu))?;
    nonzero("limits.memory_bytes", value.memory_bytes)?;
    nonzero("limits.wall", value.wall)?;
    nonzero("limits.stall_timeout", value.stall_timeout)?;
    nonzero("limits.max_output_bytes", value.max_output_bytes)
}

fn plan(value: &Plan) -> Check {
    match value {
        Plan::Remux => Ok(()),
        Plan::Transcode {
            scale_to_short_edge,
            ..
        } => within(
            "plan.scale_to_short_edge",
            u32::from(*scale_to_short_edge),
            1,
            MAX_EDGE,
        ),
    }
}

pub(crate) fn check_request(request: &Request) -> Check {
    match request {
        Request::Hello { .. } | Request::Cancel => Ok(()),
        Request::SelfCheck { forbidden_targets } => {
            within(
                "forbidden_targets",
                forbidden_targets.len(),
                0,
                MAX_FORBIDDEN_TARGETS,
            )?;
            forbidden_targets
                .iter()
                .try_for_each(|t| text("forbidden_targets[]", t, MAX_TARGET_BYTES, false))
        }
        Request::Probe { job_dir: dir } => job_dir(dir),
        Request::RenderStill {
            job_dir: dir,
            max_edge,
            poster_at_ms,
            limits: l,
        } => {
            job_dir(dir)?;
            within("max_edge", *max_edge, 1, MAX_EDGE)?;
            if let Some(at) = poster_at_ms {
                within("poster_at_ms", *at, 0, MAX_DURATION_MS)?;
            }
            limits(l)
        }
        Request::Transcode {
            job_dir: dir,
            plan: p,
            limits: l,
        } => {
            job_dir(dir)?;
            plan(p)?;
            limits(l)
        }
    }
}

fn capabilities(value: &Capabilities) -> Check {
    text("vips", &value.vips, MAX_TEXT_BYTES, false)?;
    text("ffmpeg", &value.ffmpeg, MAX_TEXT_BYTES, false)?;
    text("libheif", &value.libheif, MAX_TEXT_BYTES, false)?;
    within(
        "build_timestamp",
        value.build_timestamp,
        1,
        MAX_BUILD_TIMESTAMP,
    )?;
    within(
        "nvenc_sessions",
        value.nvenc_sessions,
        0,
        MAX_NVENC_SESSIONS,
    )
}

fn self_check(value: &SelfCheckReport) -> Check {
    within("self_check", value.0.len(), 0, MAX_SELF_CHECK_ITEMS)?;
    value
        .0
        .iter()
        .try_for_each(|item| text("self_check.name", &item.name, MAX_TEXT_BYTES, false))
}

fn video(value: &VideoStream) -> Check {
    within("video.width", value.width, 1, MAX_DIMENSION)?;
    within("video.height", value.height, 1, MAX_DIMENSION)?;
    optional_text("video.codec", value.codec.as_ref())?;
    optional_text("video.profile", value.profile.as_ref())?;
    optional_text("video.pix_fmt", value.pix_fmt.as_ref())?;
    optional_text("video.primaries", value.primaries.as_ref())?;
    optional_text("video.transfer", value.transfer.as_ref())?;
    optional_text("video.matrix", value.matrix.as_ref())?;
    if let Some(rotation) = value.rotation {
        within(
            "video.rotation",
            rotation,
            -MAX_ROTATION_DEGREES,
            MAX_ROTATION_DEGREES,
        )?;
    }
    value
        .bitrate
        .map_or(Ok(()), |b| within("video.bitrate", b, 0, MAX_BITRATE))
}

fn audio(value: &AudioStream) -> Check {
    text("audio.codec", &value.codec, MAX_TEXT_BYTES, true)
}

fn probe(value: &Probe) -> Check {
    text("container", &value.container, MAX_TEXT_BYTES, true)?;
    value.video.as_ref().map_or(Ok(()), video)?;
    value.audio.as_ref().map_or(Ok(()), audio)?;
    value
        .duration_ms
        .map_or(Ok(()), |d| within("duration_ms", d, 0, MAX_DURATION_MS))
}

fn image_result(value: &ImageResult) -> Check {
    within("width", value.width, 1, MAX_EDGE)?;
    within("height", value.height, 1, MAX_EDGE)?;
    within("bytes", value.bytes, 1, MAX_BYTES)
}

fn decode_error(value: &DecodeError) -> Check {
    match value {
        DecodeError::EncoderFailed { exit_code } => {
            within("exit_code", *exit_code, 1, MAX_EXIT_CODE)
        }
        _ => Ok(()),
    }
}

pub(crate) fn check_response(response: &Response) -> Check {
    match response {
        Response::Capabilities(c) => capabilities(c),
        Response::SelfCheck(r) => self_check(r),
        Response::Probe(p) => probe(p),
        Response::ImageResult(r) => image_result(r),
        Response::TranscodeResult(r) => {
            probe(&r.probe_of_output)?;
            within("bytes", r.bytes, 1, MAX_BYTES)
        }
        Response::Cancelled => Ok(()),
        Response::Error(e) => decode_error(e),
    }
}

pub(crate) fn check_event(event: &Event) -> Check {
    match event {
        Event::Alive => Ok(()),
        Event::Progress {
            out_time_ms, speed, ..
        } => {
            within("out_time_ms", *out_time_ms, 0, MAX_DURATION_MS)?;
            within("speed", *speed, 0.0, MAX_PROGRESS_SPEED)
        }
    }
}
