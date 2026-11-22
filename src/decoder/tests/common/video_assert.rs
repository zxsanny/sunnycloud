//! The checks every transcode output must pass, shared by the host-process cases and the container
//! suite. The reference is always an independent ffprobe of the output file.

use std::fs;
use std::path::Path;

use media_shared::decoder_protocol::TranscodeResult;
use media_shared::policy::Probe;

use super::video_ref::{
    moov_before_mdat, packet_md5, report_of, streams_of, tag_keys, video_packets,
};

pub fn seconds(report: &serde_json::Value) -> f64 {
    report["format"]["duration"]
        .as_str()
        .expect("duration")
        .parse()
        .expect("seconds")
}

pub fn dims(report: &serde_json::Value) -> (u64, u64) {
    let video = streams_of(report, "video")[0];
    (
        video["width"].as_u64().expect("width"),
        video["height"].as_u64().expect("height"),
    )
}

/// AC-2: the probe the runner reports is what an independent ffprobe reads from the same file.
pub fn assert_probe_is_real(reported: &Probe, output: &Path) {
    let report = report_of(output);
    let video = streams_of(&report, "video")[0];
    let stream = reported.video.as_ref().expect("video facts");
    let text_of = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    assert_eq!(stream.codec, text_of(&video["codec_name"]));
    assert_eq!(stream.profile, text_of(&video["profile"]));
    assert_eq!(u64::from(stream.width), video["width"].as_u64().expect("w"));
    assert_eq!(
        u64::from(stream.height),
        video["height"].as_u64().expect("h")
    );
    assert_eq!(stream.pix_fmt, text_of(&video["pix_fmt"]));
    assert_eq!(stream.primaries, text_of(&video["color_primaries"]));
    assert_eq!(stream.transfer, text_of(&video["color_transfer"]));
    assert_eq!(stream.matrix, text_of(&video["color_space"]));
    assert_eq!(
        reported.audio.as_ref().map(|audio| audio.codec.clone()),
        streams_of(&report, "audio")
            .first()
            .and_then(|audio| text_of(&audio["codec_name"]))
    );
    assert_eq!(reported.moov_first, moov_before_mdat(output));
    let independent = seconds(&report) * 1000.0;
    let reported_ms = reported.duration_ms.expect("duration") as f64;
    assert!(
        (reported_ms - independent).abs() <= 1.0,
        "{reported_ms} vs {independent}"
    );
}

/// The properties every CPU transcode must have, whatever the input.
pub fn assert_compliant(
    output: &Path,
    result: &TranscodeResult,
    input: &Path,
    expected: (u64, u64),
    audio: bool,
) {
    let report = report_of(output);
    let video = streams_of(&report, "video");
    assert_eq!(video.len(), 1, "exactly one video stream");
    let video = video[0];
    assert_eq!(video["codec_name"], "h264");
    assert_eq!(video["profile"], "High");
    assert_eq!(video["pix_fmt"], "yuv420p");
    for tag in ["color_primaries", "color_transfer", "color_space"] {
        assert_eq!(video[tag], "bt709", "{tag}");
    }
    assert_eq!(dims(&report), expected);
    assert!(expected.0.is_multiple_of(2) && expected.1.is_multiple_of(2));
    assert!(expected.0.min(expected.1) <= 1080 && expected.0.max(expected.1) <= 1920);
    assert!(
        streams_of(&report, "video")[0]["side_data_list"].is_null(),
        "no rotation tag is left to rotate twice"
    );
    let audios = streams_of(&report, "audio");
    assert_eq!(audios.len(), usize::from(audio));
    if let Some(aac) = audios.first() {
        assert_eq!(aac["codec_name"], "aac");
        assert_eq!(aac["channels"], 2);
        let rate: u64 = aac["bit_rate"]
            .as_str()
            .expect("bit_rate")
            .parse()
            .expect("rate");
        // An encoder spends little on near-silence, so only the upper bound holds for any clip.
        assert!(rate <= 150_000, "aac bit rate {rate}");
    }
    assert!(streams_of(&report, "subtitle").is_empty());
    assert!(streams_of(&report, "data").is_empty());
    assert!(moov_before_mdat(output), "moov must come before mdat");
    let gap = (seconds(&report) - seconds(&report_of(input))).abs();
    assert!(gap <= 0.15, "duration differs by {gap} s");
    assert!(result.bytes > 0 && result.bytes == fs::metadata(output).expect("stat").len());
    assert_probe_is_real(&result.probe_of_output, output);
}

/// IT-10-11: a remux copies the packets of the first video and audio streams bit for bit, keeps
/// codec, profile, size and bitrate, puts `moov` first, and drops subtitles, data streams,
/// chapters and global metadata.
pub fn assert_remux(input: &Path, output: &Path, result: &TranscodeResult) {
    let (before, after) = (report_of(input), report_of(output));
    assert!(!moov_before_mdat(input) && moov_before_mdat(output));
    for map in ["0:v:0", "0:a:0"] {
        assert_eq!(
            packet_md5(input, map),
            packet_md5(output, map),
            "{map} packets"
        );
    }
    assert_eq!(video_packets(input), video_packets(output));
    let (old, new) = (
        streams_of(&before, "video")[0],
        streams_of(&after, "video")[0],
    );
    for field in [
        "codec_name",
        "profile",
        "width",
        "height",
        "bit_rate",
        "pix_fmt",
    ] {
        assert_eq!(old[field], new[field], "{field}");
    }
    assert!(streams_of(&after, "subtitle").is_empty());
    assert!(streams_of(&after, "data").is_empty());
    assert!(after["chapters"].as_array().expect("chapters").is_empty());
    let forbidden = ["title", "location"];
    assert!(
        tag_keys(&after)
            .iter()
            .all(|key| !forbidden.contains(&key.as_str()))
    );
    assert!(result.probe_of_output.moov_first);
    assert_probe_is_real(&result.probe_of_output, output);
}
