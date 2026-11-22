//! The image executor (E-19/c08, 10 section 4): sniff, read the header, refuse an oversize
//! image before any decode, render one WebP with `vips thumbnail`, read the result back.
//!
//! libvips is used as a CLI only. The input reaches a child as `/proc/self/fd/9`, so no job
//! path is an argument, and the output name is relative to the child's working directory
//! `out/`.

use std::fs::File;
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use media_shared::decoder_protocol::{DecodeError, ImageResult, LimitKind, Response};
use media_shared::policy::{MediaKind, Probe, VideoStream};

use crate::exec::{Exec, internal};
use crate::failure::Failure;
use crate::ffprobe;
use crate::params::Params;
use crate::poster;
use crate::process::input_path;
use crate::sniff::{Profile, Sniffed, read_head, sniff};
use crate::sys::{O_NOFOLLOW, O_NONBLOCK};

const RESULT: &str = "result.webp";
const MAX_WEBP_EDGE: u64 = 16_383;

pub(crate) fn render_still(
    exec: &Exec<'_>,
    params: &Params,
    max_edge: u32,
    poster_at_ms: u64,
) -> Result<Response, Failure> {
    let input = exec.context.input();
    match sniffed(input)? {
        Sniffed::Still { profile, .. } => render_from(exec, params, input, profile, max_edge),
        Sniffed::Video => poster::render(exec, params, max_edge, poster_at_ms),
        Sniffed::Unknown => Err(DecodeError::Unsupported.into()),
    }
}

/// Refuses an oversize image from its header, then renders `input` as one WebP and reads the
/// result back. `input` is the job's own file or a PNG the runner itself made.
pub(crate) fn render_from(
    exec: &Exec<'_>,
    params: &Params,
    input: &File,
    profile: Profile,
    max_edge: u32,
) -> Result<Response, Failure> {
    let (width, height) = dimensions(exec, profile, input)?;
    if width.saturating_mul(height) > exec.config.photo_max_pixels {
        return Err(Failure::limit(LimitKind::Pixels));
    }
    thumbnail(exec, params, profile, input, max_edge)?;
    read_back(exec).map(Response::ImageResult)
}

pub(crate) fn probe(exec: &Exec<'_>) -> Result<Response, Failure> {
    let input = exec.context.input();
    let (profile, format) = match sniffed(input)? {
        Sniffed::Still { profile, format } => (profile, format),
        Sniffed::Video => return ffprobe::probe(exec).map(Response::Probe),
        Sniffed::Unknown => return Err(DecodeError::Unsupported.into()),
    };
    let (width, height) = dimensions(exec, profile, input)?;
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(Failure::limit(LimitKind::Pixels));
    };
    Ok(Response::Probe(Probe {
        kind: MediaKind::Photo,
        container: format.to_owned(),
        moov_first: false,
        video: Some(VideoStream {
            codec: None,
            profile: None,
            width,
            height,
            rotation: None,
            bitrate: None,
            pix_fmt: None,
            primaries: None,
            transfer: None,
            matrix: None,
        }),
        audio: None,
        duration_ms: None,
    }))
}

pub(crate) fn sniffed(input: &File) -> Result<Sniffed, Failure> {
    let head = read_head(input).map_err(|error| internal("cannot read the input", &error))?;
    Ok(sniff(&head))
}

/// The environment of a vips child: exactly the profile's allowlist.
fn vips_env(cpu: u32, profile: Profile) -> Vec<(&'static str, String)> {
    let mut env = vec![("VIPS_CONCURRENCY", cpu.to_string())];
    if profile == Profile::A {
        env.push(("VIPS_BLOCK_UNTRUSTED", "1".to_owned()));
    }
    env
}

/// One field per call: libvips 8.15 prints only the last of several `-f` options, 8.18 prints all.
fn header_args(field: &str) -> Vec<String> {
    ["-f", field]
        .into_iter()
        .map(str::to_owned)
        .chain([input_path()])
        .collect()
}

/// `vips thumbnail` is the same operation `vipsthumbnail` runs, but its flags are the same on
/// libvips 8.15 and 8.18: `vipsthumbnail` renamed `-e/-o` to `--output-profile/--path` and an
/// 8.15 `-o` needs the output directory in the argument. The output name is relative to the
/// child's working directory `out/`.
fn thumbnail_args(max_edge: u32, webp_quality: u32) -> Vec<String> {
    let edge = max_edge.to_string();
    [
        "thumbnail",
        &input_path(),
        &format!("{RESULT}[Q={webp_quality},keep=none]"),
        &edge,
        "--height",
        &edge,
        // Only shrink: a small image is not enlarged to the edge.
        "--size",
        "down",
        "--export-profile",
        "srgb",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Width and height of the stored image, from the header alone.
fn dimensions(exec: &Exec<'_>, profile: Profile, file: &File) -> Result<(u64, u64), Failure> {
    Ok((
        header_field(exec, profile, file, "width")?,
        header_field(exec, profile, file, "height")?,
    ))
}

fn header_field(
    exec: &Exec<'_>,
    profile: Profile,
    file: &File,
    field: &str,
) -> Result<u64, Failure> {
    let out = exec.checked_out_dir()?;
    let finished = exec.run(
        "vipsheader",
        &exec.tools.vipsheader,
        header_args(field),
        vips_env(exec.limits.cpu, profile),
        &out,
        file,
    )?;
    parse_dimension(&finished.stdout).ok_or_else(|| DecodeError::Corrupt.into())
}

fn parse_dimension(stdout: &[u8]) -> Option<u64> {
    let value: u64 = std::str::from_utf8(stdout).ok()?.trim().parse().ok()?;
    (value > 0).then_some(value)
}

fn thumbnail(
    exec: &Exec<'_>,
    params: &Params,
    profile: Profile,
    input: &File,
    max_edge: u32,
) -> Result<(), Failure> {
    let out = exec.checked_out_dir()?;
    // A leftover name could be a symlink that vips would write through; removing it removes the link.
    match std::fs::remove_file(out.join(RESULT)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(internal("cannot clear out/result.webp", &error));
        }
        _ => {}
    }
    exec.run(
        "vips",
        &exec.tools.vips,
        thumbnail_args(max_edge, params.webp_quality),
        vips_env(exec.limits.cpu, profile),
        &out,
        input,
    )?;
    Ok(())
}

/// The output must be a regular single-link WebP within the size limit; its width and height
/// are read from the file itself, not assumed.
fn read_back(exec: &Exec<'_>) -> Result<ImageResult, Failure> {
    let out = exec.checked_out_dir()?;
    let file = open_result(&out.join(RESULT))?;
    let meta = file
        .metadata()
        .map_err(|error| internal("cannot stat the output", &error))?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() == 0 {
        return Err(DecodeError::OutputInvalid.into());
    }
    if meta.len() > exec.limits.max_output_bytes {
        return Err(Failure::limit(LimitKind::Output));
    }
    let head = read_head(&file).map_err(|error| internal("cannot read the output", &error))?;
    if !matches!(sniff(&head), Sniffed::Still { format: "webp", .. }) {
        return Err(DecodeError::OutputInvalid.into());
    }
    let (width, height) = dimensions(exec, Profile::A, &file)?;
    match (u32::try_from(width), u32::try_from(height)) {
        (Ok(w), Ok(h)) if width <= MAX_WEBP_EDGE && height <= MAX_WEBP_EDGE => Ok(ImageResult {
            width: w,
            height: h,
            bytes: meta.len(),
        }),
        _ => Err(DecodeError::OutputInvalid.into()),
    }
}

pub(crate) fn open_result(path: &Path) -> Result<File, Failure> {
    File::options()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(path)
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => DecodeError::OutputInvalid.into(),
            _ => internal("cannot open the output", &error),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_decimal_line_is_a_dimension() {
        assert_eq!(parse_dimension(b"400\n"), Some(400));
        assert_eq!(parse_dimension(b"  7 "), Some(7));
    }

    #[test]
    fn anything_else_is_not_a_dimension() {
        let cases: [&[u8]; 6] = [b"", b"0\n", b"a\n", b"4 3\n", b"-1\n", b"\xff\n"];

        for case in cases {
            assert_eq!(parse_dimension(case), None, "{case:?}");
        }
    }

    #[test]
    fn the_thumbnail_argv_is_fixed_and_carries_no_job_path() {
        let args = thumbnail_args(320, 80);

        assert_eq!(
            args,
            [
                "thumbnail",
                "/proc/self/fd/9",
                "result.webp[Q=80,keep=none]",
                "320",
                "--height",
                "320",
                "--size",
                "down",
                "--export-profile",
                "srgb"
            ]
        );
    }

    #[test]
    fn the_webp_quality_is_the_one_the_caller_passes_from_the_params_table() {
        let quality = crate::params::for_version(1)
            .expect("version 1")
            .webp_quality;

        let args = thumbnail_args(320, quality);
        let other = thumbnail_args(320, 55);

        assert_eq!(args[2], "result.webp[Q=80,keep=none]");
        assert_eq!(other[2], "result.webp[Q=55,keep=none]");
    }

    #[test]
    fn profile_a_blocks_untrusted_loaders_and_profile_b_does_not() {
        let a = vips_env(2, Profile::A);
        let b = vips_env(2, Profile::B);

        assert_eq!(
            a,
            [
                ("VIPS_CONCURRENCY", "2".to_owned()),
                ("VIPS_BLOCK_UNTRUSTED", "1".to_owned())
            ]
        );
        assert_eq!(b, [("VIPS_CONCURRENCY", "2".to_owned())]);
    }

    #[test]
    fn the_header_argv_reads_one_field_of_the_fixed_input_path() {
        assert_eq!(header_args("width"), ["-f", "width", "/proc/self/fd/9"]);
        assert_eq!(header_args("height"), ["-f", "height", "/proc/self/fd/9"]);
    }
}
