//! The video poster path (E-19/c10, 10 section 4): ffmpeg decodes the one frame nearest
//! `poster_at_ms` into a PNG in `work/`, tone-mapped when the clip is HDR; that PNG, which the
//! runner itself made, then goes through the image profile A to the WebP. The untrusted
//! container never reaches vips.

use std::fs::File;
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use media_shared::decoder_protocol::{DecodeError, Response};
use media_shared::policy::VideoStream;

use crate::exec::{Exec, internal};
use crate::failure::Failure;
use crate::ffprobe::{self, input_options};
use crate::image;
use crate::process::input_path;
use crate::sniff::{Profile, Sniffed, read_head, sniff};
use crate::sys::{O_NOFOLLOW, O_NONBLOCK};

const FRAME: &str = "poster.png";
const HDR_TRANSFERS: [&str; 2] = ["smpte2084", "arib-std-b67"];

pub(crate) fn render(
    exec: &Exec<'_>,
    max_edge: u32,
    poster_at_ms: u64,
) -> Result<Response, Failure> {
    let facts = ffprobe::probe(exec)?;
    let hdr = facts.video.as_ref().is_some_and(is_hdr);
    exec.run(
        "ffmpeg",
        &exec.tools.ffmpeg,
        frame_args(max_edge, poster_at_ms, hdr, exec.limits.cpu),
        Vec::new(),
        exec.work.path(),
        exec.context.input(),
    )?;
    let frame = open_frame(exec)?;
    image::render_from(exec, &frame, Profile::A, max_edge)
}

fn is_hdr(video: &VideoStream) -> bool {
    video
        .transfer
        .as_deref()
        .is_some_and(|transfer| HDR_TRANSFERS.contains(&transfer))
}

/// Exactly one decoded frame, first video stream only, no audio, subtitles or data, and no
/// metadata copied. The output name is relative to the child's working directory `work/`.
fn frame_args(max_edge: u32, at_ms: u64, hdr: bool, cpu: u32) -> Vec<String> {
    let mut args: Vec<String> = ["-nostdin", "-hide_banner", "-loglevel", "error"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    args.extend(input_options());
    args.extend([
        "-threads".to_owned(),
        cpu.to_string(),
        "-filter_threads".to_owned(),
        cpu.to_string(),
        "-ss".to_owned(),
        format!("{}.{:03}", at_ms / 1000, at_ms % 1000),
        "-i".to_owned(),
        input_path(),
        "-map".to_owned(),
        "0:v:0".to_owned(),
        "-an".to_owned(),
        "-sn".to_owned(),
        "-dn".to_owned(),
        "-map_metadata".to_owned(),
        "-1".to_owned(),
        "-frames:v".to_owned(),
        "1".to_owned(),
        "-vf".to_owned(),
        filter(max_edge, hdr),
        "-y".to_owned(),
        FRAME.to_owned(),
    ]);
    args
}

/// The downscale comes first: tone mapping at full size costs two to three times more (S5).
/// The HDR chain is the one fixed by gate S5 (10 section 13.2).
fn filter(max_edge: u32, hdr: bool) -> String {
    let scale = format!(
        "scale=w='min(iw,{max_edge})':h='min(ih,{max_edge})':force_original_aspect_ratio=decrease:flags=bicubic"
    );
    if hdr {
        format!(
            "{scale},zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p,format=rgb24"
        )
    } else {
        scale
    }
}

/// The frame ffmpeg wrote: a regular single-link PNG, checked by its bytes.
fn open_frame(exec: &Exec<'_>) -> Result<File, Failure> {
    let file = File::options()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(exec.work.path().join(FRAME))
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => DecodeError::OutputInvalid.into(),
            _ => internal("cannot open the poster frame", &error),
        })?;
    let meta = file
        .metadata()
        .map_err(|error| internal("cannot stat the poster frame", &error))?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() == 0 {
        return Err(DecodeError::OutputInvalid.into());
    }
    let head =
        read_head(&file).map_err(|error| internal("cannot read the poster frame", &error))?;
    if matches!(sniff(&head), Sniffed::Still { format: "png", .. }) {
        Ok(file)
    } else {
        Err(DecodeError::OutputInvalid.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_grab_argv_is_fixed_and_carries_no_job_path() {
        let args = frame_args(320, 1500, false, 2);

        assert_eq!(
            args,
            [
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-protocol_whitelist",
                "file",
                "-format_whitelist",
                "mov,matroska,avi,mpegts",
                "-codec_whitelist",
                "h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt",
                "-threads",
                "2",
                "-filter_threads",
                "2",
                "-ss",
                "1.500",
                "-i",
                "/proc/self/fd/9",
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-map_metadata",
                "-1",
                "-frames:v",
                "1",
                "-vf",
                "scale=w='min(iw,320)':h='min(ih,320)':force_original_aspect_ratio=decrease:flags=bicubic",
                "-y",
                "poster.png"
            ]
        );
    }

    #[test]
    fn an_hdr_clip_gets_the_tone_mapping_chain_after_the_downscale_and_an_sdr_clip_does_not() {
        let hdr = filter(320, true);
        let sdr = filter(320, false);

        assert!(hdr.starts_with(&sdr));
        assert!(hdr.contains("tonemap=tonemap=hable:desat=0"));
        assert!(!sdr.contains("tonemap"));
    }

    #[test]
    fn the_offset_is_written_as_seconds_with_milliseconds() {
        let seek = |at_ms| {
            let args = frame_args(320, at_ms, false, 1);
            let at = args.iter().position(|arg| arg == "-ss").expect("-ss");
            args[at + 1].clone()
        };

        assert_eq!(seek(0), "0.000");
        assert_eq!(seek(900), "0.900");
        assert_eq!(seek(61_007), "61.007");
    }
}
