//! The image executor (E-19/c08, 10 section 4): sniff, read the header, refuse an oversize
//! image before any decode, render one WebP with `vipsthumbnail`, read the result back.
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
use crate::poster;
use crate::process::input_path;
use crate::sniff::{Profile, Sniffed, read_head, sniff};
use crate::sys::{O_NOFOLLOW, O_NONBLOCK};

/// The quality of every WebP. The protocol carries no `params_version` for `render_still`, so
/// the value is fixed here until the contract names where it comes from (AZ-3857 flagged
/// concern, decoder protocol open point 7).
const WEBP_QUALITY: u32 = 80;
const RESULT: &str = "result.webp";
const MAX_WEBP_EDGE: u64 = 16_383;

pub(crate) fn render_still(
    exec: &Exec<'_>,
    max_edge: u32,
    poster_at_ms: u64,
) -> Result<Response, Failure> {
    let input = exec.context.input();
    match sniffed(input)? {
        Sniffed::Still { profile, .. } => render_from(exec, input, profile, max_edge),
        Sniffed::Video => poster::render(exec, max_edge, poster_at_ms),
        Sniffed::Unknown => Err(DecodeError::Unsupported.into()),
    }
}

/// Refuses an oversize image from its header, then renders `input` as one WebP and reads the
/// result back. `input` is the job's own file or a PNG the runner itself made.
pub(crate) fn render_from(
    exec: &Exec<'_>,
    input: &File,
    profile: Profile,
    max_edge: u32,
) -> Result<Response, Failure> {
    let (width, height) = dimensions(exec, profile, input)?;
    if width.saturating_mul(height) > exec.config.photo_max_pixels {
        return Err(Failure::limit(LimitKind::Pixels));
    }
    thumbnail(exec, profile, input, max_edge)?;
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

fn sniffed(input: &File) -> Result<Sniffed, Failure> {
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

fn header_args() -> Vec<String> {
    ["-f", "width", "-f", "height"]
        .into_iter()
        .map(str::to_owned)
        .chain([input_path()])
        .collect()
}

fn thumbnail_args(max_edge: u32) -> Vec<String> {
    vec![
        input_path(),
        "--size".to_owned(),
        // The trailing `>` shrinks only: a small image is not enlarged to the edge.
        format!("{max_edge}x{max_edge}>"),
        "--output-profile".to_owned(),
        "srgb".to_owned(),
        "--path".to_owned(),
        format!("{RESULT}[Q={WEBP_QUALITY},keep=none]"),
    ]
}

/// Width and height of the stored image, from the header alone.
fn dimensions(exec: &Exec<'_>, profile: Profile, file: &File) -> Result<(u64, u64), Failure> {
    let out = exec.checked_out_dir()?;
    let finished = exec.run(
        "vipsheader",
        &exec.tools.vipsheader,
        header_args(),
        vips_env(exec.limits.cpu, profile),
        &out,
        file,
    )?;
    parse_dimensions(&finished.stdout).ok_or_else(|| DecodeError::Corrupt.into())
}

fn parse_dimensions(stdout: &[u8]) -> Option<(u64, u64)> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut lines = text.lines();
    let width = lines.next()?.trim().parse().ok()?;
    let height = lines.next()?.trim().parse().ok()?;
    match lines.next() {
        None if width > 0 && height > 0 => Some((width, height)),
        _ => None,
    }
}

fn thumbnail(
    exec: &Exec<'_>,
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
        "vipsthumbnail",
        &exec.tools.vipsthumbnail,
        thumbnail_args(max_edge),
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

fn open_result(path: &Path) -> Result<File, Failure> {
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
    fn two_decimal_lines_are_the_dimensions() {
        assert_eq!(parse_dimensions(b"400\n300\n"), Some((400, 300)));
    }

    #[test]
    fn anything_else_is_not_dimensions() {
        let cases: [&[u8]; 6] = [
            b"",
            b"400\n",
            b"400\n300\n5\n",
            b"0\n3\n",
            b"a\nb\n",
            b"\xff\n1\n",
        ];

        for case in cases {
            assert_eq!(parse_dimensions(case), None, "{case:?}");
        }
    }

    #[test]
    fn the_thumbnail_argv_is_fixed_and_carries_no_job_path() {
        let args = thumbnail_args(320);

        assert_eq!(
            args,
            [
                "/proc/self/fd/9",
                "--size",
                "320x320>",
                "--output-profile",
                "srgb",
                "--path",
                "result.webp[Q=80,keep=none]"
            ]
        );
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
    fn the_header_argv_reads_width_and_height_of_the_fixed_input_path() {
        assert_eq!(
            header_args(),
            ["-f", "width", "-f", "height", "/proc/self/fd/9"]
        );
    }
}
