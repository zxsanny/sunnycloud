//! The video probe (E-19/c09): `ffprobe` under the S5 whitelists, parsed into facts, plus the
//! position of `moov` from a bounded scan of top-level MP4 box headers. The decoder reports
//! facts and never judges compliance.

use std::io;
use std::os::unix::fs::FileExt;

use media_shared::decoder_protocol::DecodeError;
use media_shared::decoder_protocol::ranges::{MAX_BITRATE, MAX_DIMENSION, MAX_DURATION_MS};
use media_shared::policy::{AudioStream, MediaKind, Probe, VideoStream};
use serde_json::Value;

use crate::exec::{Exec, internal};
use crate::failure::Failure;
use crate::process::input_path;

/// Fixed by gate S5 (10 section 13.1). The codec list names decoders, not codecs.
const FORMAT_WHITELIST: &str = "mov,matroska,avi,mpegts";
const CODEC_WHITELIST: &str = "h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt";
const MAX_TOP_LEVEL_BOXES: usize = 64;
const MAX_TEXT: usize = 128;

/// The options every ffmpeg and ffprobe call starts with: only the `file` protocol and only the
/// listed demuxers and decoders, so a playlist or a concat file cannot name another file.
pub(crate) fn input_options() -> Vec<String> {
    [
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        FORMAT_WHITELIST,
        "-codec_whitelist",
        CODEC_WHITELIST,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// `ffprobe` does not take `-nostdin`; its stdin is null anyway.
fn probe_args() -> Vec<String> {
    let mut args: Vec<String> = [
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.extend(input_options());
    args.extend(["-i".to_owned(), input_path()]);
    args
}

pub(crate) fn probe(exec: &Exec<'_>) -> Result<Probe, Failure> {
    let input = exec.context.input();
    let finished = exec.run(
        "ffprobe",
        &exec.tools.ffprobe,
        probe_args(),
        Vec::new(),
        exec.work.path(),
        input,
    )?;
    let mut probe = parse_probe(&finished.stdout)?;
    if is_mp4(&probe.container) {
        probe.moov_first = moov_first(|offset, buffer| input.read_at(buffer, offset))
            .map_err(|error| internal("cannot scan the box headers", &error))?;
    }
    Ok(probe)
}

fn is_mp4(container: &str) -> bool {
    container.split(',').any(|name| name == "mp4")
}

/// A report that is not JSON, or has no usable video stream dimensions, is a damaged file; a
/// file without a video stream is a kind this runner does not process.
fn parse_probe(stdout: &[u8]) -> Result<Probe, Failure> {
    let report: Value = serde_json::from_slice(stdout).map_err(|_| DecodeError::Corrupt)?;
    let streams = report["streams"].as_array().ok_or(DecodeError::Corrupt)?;
    let of_type = |kind: &str| {
        streams
            .iter()
            .find(|stream| stream["codec_type"] == kind && !is_cover_art(stream))
    };
    let video = of_type("video").ok_or(DecodeError::Unsupported)?;
    let format = &report["format"];
    Ok(Probe {
        kind: MediaKind::Video,
        container: text(&format["format_name"]).ok_or(DecodeError::Corrupt)?,
        moov_first: false,
        video: Some(video_stream(video)?),
        audio: of_type("audio")
            .and_then(|stream| text(&stream["codec_name"]))
            .map(|codec| AudioStream { codec }),
        duration_ms: duration_ms(&format["duration"]),
    })
}

fn is_cover_art(stream: &Value) -> bool {
    stream["disposition"]["attached_pic"] == 1
}

fn video_stream(stream: &Value) -> Result<VideoStream, DecodeError> {
    let dimension = |key: &str| {
        number(&stream[key])
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| (1..=MAX_DIMENSION).contains(value))
            .ok_or(DecodeError::Corrupt)
    };
    Ok(VideoStream {
        codec: text(&stream["codec_name"]),
        profile: text(&stream["profile"]),
        width: dimension("width")?,
        height: dimension("height")?,
        rotation: rotation(stream),
        bitrate: number(&stream["bit_rate"]).filter(|bps| *bps <= MAX_BITRATE),
        pix_fmt: text(&stream["pix_fmt"]),
        primaries: text(&stream["color_primaries"]),
        transfer: text(&stream["color_transfer"]),
        matrix: text(&stream["color_space"]),
    })
}

/// A JSON number, or a decimal string (ffprobe prints some integers as strings).
fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

/// A non-empty printable ASCII text within the protocol bound; anything else is "not reported".
fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| {
            !text.is_empty()
                && text.len() <= MAX_TEXT
                && text.bytes().all(|byte| (0x20..0x7f).contains(&byte))
        })
        .map(str::to_owned)
}

/// The display-matrix angle (`side_data_list`), or the legacy `rotate` tag.
fn rotation(stream: &Value) -> Option<i32> {
    let from_matrix = stream["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|data| data["rotation"].as_f64()));
    let degrees = from_matrix.or_else(|| stream["tags"]["rotate"].as_str()?.parse().ok())?;
    let rounded = degrees.round();
    (rounded.abs() <= 360.0).then_some(rounded as i32)
}

fn duration_ms(value: &Value) -> Option<u64> {
    let seconds: f64 = value.as_str()?.parse().ok()?;
    let millis = (seconds * 1000.0).round();
    (millis.is_finite() && millis >= 0.0 && millis <= MAX_DURATION_MS as f64)
        .then_some(millis as u64)
}

/// True when a `moov` box comes before the first `mdat` among the first 64 top-level boxes.
/// Header bytes only; no box is read. No `moov` within the scan is `false`, the safe value for
/// the remux rule.
fn moov_first(mut read_at: impl FnMut(u64, &mut [u8]) -> io::Result<usize>) -> io::Result<bool> {
    let mut offset = 0u64;
    for _ in 0..MAX_TOP_LEVEL_BOXES {
        let mut header = [0u8; 16];
        let mut have = 0;
        while have < header.len() {
            let read = read_at(offset + have as u64, &mut header[have..])?;
            if read == 0 {
                break;
            }
            have += read;
        }
        if have < 8 {
            return Ok(false);
        }
        match &header[4..8] {
            b"moov" => return Ok(true),
            b"mdat" => return Ok(false),
            _ => {}
        }
        let size = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let length = match size {
            0 => return Ok(false),
            1 if have == 16 => u64::from_be_bytes(header[8..16].try_into().unwrap_or([0; 8])),
            1 => return Ok(false),
            _ => size,
        };
        if length < 8 {
            return Ok(false);
        }
        let Some(next) = offset.checked_add(length) else {
            return Ok(false);
        };
        offset = next;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(list: &[(&[u8; 4], u32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (kind, size) in list {
            bytes.extend_from_slice(&size.to_be_bytes());
            bytes.extend_from_slice(*kind);
            bytes.resize(bytes.len() + (*size as usize).saturating_sub(8), 0);
        }
        bytes
    }

    /// Scans `bytes` and counts the header reads.
    fn scan(bytes: &[u8]) -> (bool, usize) {
        let mut reads = 0;
        let found = moov_first(|offset, buffer| {
            reads += 1;
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(bytes.len());
            let count = buffer.len().min(bytes.len() - start);
            buffer[..count].copy_from_slice(&bytes[start..start + count]);
            Ok(count)
        })
        .expect("scan");
        (found, reads)
    }

    #[test]
    fn moov_before_mdat_is_first_and_after_it_is_not() {
        let first = boxes(&[(b"ftyp", 24), (b"moov", 100), (b"mdat", 50)]);
        let last = boxes(&[(b"ftyp", 24), (b"free", 16), (b"mdat", 50), (b"moov", 100)]);

        assert!(scan(&first).0);
        assert!(!scan(&last).0);
    }

    #[test]
    fn a_file_with_more_than_64_boxes_stops_the_scan_at_64_headers() {
        let list: Vec<(&[u8; 4], u32)> = std::iter::repeat_n((b"free", 8u32), 80).collect();
        let mut many = boxes(&list);
        many.extend(boxes(&[(b"moov", 8)]));

        let (found, reads) = scan(&many);

        assert!(!found);
        assert_eq!(reads, 64);
    }

    #[test]
    fn a_truncated_header_a_zero_size_and_a_size_below_the_header_end_the_scan() {
        let cases: [&[u8]; 4] = [
            b"\0\0\0\x18ft",
            &[
                0, 0, 0, 0, b'f', b'r', b'e', b'e', 0, 0, 0, 8, b'm', b'o', b'o', b'v',
            ],
            &[
                0, 0, 0, 4, b'f', b'r', b'e', b'e', 0, 0, 0, 8, b'm', b'o', b'o', b'v',
            ],
            &[],
        ];

        for case in cases {
            assert!(!scan(case).0, "{case:?}");
        }
    }

    #[test]
    fn a_64_bit_box_size_is_followed() {
        let mut bytes = vec![0, 0, 0, 1, b'f', b'r', b'e', b'e'];
        bytes.extend_from_slice(&24u64.to_be_bytes());
        bytes.resize(24, 0);
        bytes.extend(boxes(&[(b"moov", 8)]));

        assert!(scan(&bytes).0);
    }

    const REPORT: &str = r#"{
        "streams": [
          {"codec_type":"video","codec_name":"hevc","profile":"Main 10","width":3840,"height":2160,
           "pix_fmt":"yuv420p10le","bit_rate":"25000000","color_primaries":"bt2020",
           "color_transfer":"arib-std-b67","color_space":"bt2020nc",
           "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]},
          {"codec_type":"audio","codec_name":"aac"}],
        "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"12.345000"}}"#;

    #[test]
    fn a_report_is_mapped_field_by_field() {
        let probe = parse_probe(REPORT.as_bytes()).expect("parsed");

        let video = probe.video.expect("video");
        assert_eq!(probe.kind, MediaKind::Video);
        assert_eq!(probe.container, "mov,mp4,m4a,3gp,3g2,mj2");
        assert_eq!(probe.duration_ms, Some(12_345));
        assert_eq!(
            probe.audio,
            Some(AudioStream {
                codec: "aac".into()
            })
        );
        assert_eq!((video.width, video.height), (3840, 2160));
        assert_eq!(video.codec.as_deref(), Some("hevc"));
        assert_eq!(video.profile.as_deref(), Some("Main 10"));
        assert_eq!(video.rotation, Some(-90));
        assert_eq!(video.bitrate, Some(25_000_000));
        assert_eq!(video.transfer.as_deref(), Some("arib-std-b67"));
        assert_eq!(video.primaries.as_deref(), Some("bt2020"));
        assert_eq!(video.matrix.as_deref(), Some("bt2020nc"));
    }

    #[test]
    fn unreported_values_stay_unreported_and_cover_art_is_not_the_video() {
        let report = r#"{"streams":[
            {"codec_type":"video","codec_name":"mjpeg","width":10,"height":10,"disposition":{"attached_pic":1}},
            {"codec_type":"video","codec_name":"h264","width":320,"height":240}],
            "format":{"format_name":"matroska,webm"}}"#;

        let probe = parse_probe(report.as_bytes()).expect("parsed");

        let video = probe.video.expect("video");
        assert_eq!(video.codec.as_deref(), Some("h264"));
        assert_eq!(
            (video.bitrate, video.rotation, video.transfer),
            (None, None, None)
        );
        assert_eq!((probe.audio, probe.duration_ms), (None, None));
    }

    #[test]
    fn damaged_and_video_less_reports_are_typed_failures() {
        let no_video = r#"{"streams":[{"codec_type":"audio","codec_name":"aac"}],"format":{"format_name":"mov"}}"#;
        let no_size = r#"{"streams":[{"codec_type":"video","codec_name":"h264"}],"format":{"format_name":"mov"}}"#;
        let cases = [
            ("", DecodeError::Corrupt),
            ("{\"streams\":", DecodeError::Corrupt),
            ("{}", DecodeError::Corrupt),
            (no_video, DecodeError::Unsupported),
            (no_size, DecodeError::Corrupt),
        ];

        for (report, expected) in cases {
            assert_eq!(
                parse_probe(report.as_bytes()).err(),
                Some(Failure::Decode(expected)),
                "{report}"
            );
        }
    }

    #[test]
    fn every_ffprobe_call_carries_the_three_whitelist_options_and_the_fixed_input() {
        let args = probe_args();

        let joined = args.join(" ");
        assert!(joined.contains("-protocol_whitelist file"), "{joined}");
        assert!(
            joined.contains("-format_whitelist mov,matroska,avi,mpegts"),
            "{joined}"
        );
        assert!(
            joined.contains(
                "-codec_whitelist h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt"
            ),
            "{joined}"
        );
        assert!(args.ends_with(&["-i".to_owned(), "/proc/self/fd/9".to_owned()]));
        assert!(!args.iter().any(|arg| arg == "-nostdin"));
    }
}
