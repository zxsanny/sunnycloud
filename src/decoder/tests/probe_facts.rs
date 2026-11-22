//! AZ-3858 through the real runner binary and the host's real ffprobe: every field of a video
//! probe against an independent ffprobe of the same file, and the hostile inputs of AC-5: the
//! tool is run with the whitelist options and the fixed input only, and no byte of the file they
//! name reaches a reply or the log.
mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::media::{Media, MediaRunner, path_with_scripts, probe, real_tool};
use media_shared::decoder_protocol::{DecodeError, Response};
use media_shared::policy::{MediaKind, Probe};
use serde_json::Value;
use tempfile::TempDir;

type Make = fn(&Media) -> Vec<u8>;

fn probed(runner: &mut MediaRunner, bytes: &[u8]) -> Probe {
    let mut client = runner.connect();
    let job = runner.job(bytes);
    match probe(&mut client, &job) {
        Response::Probe(facts) => facts,
        other => panic!("expected a probe, got {other:?}"),
    }
}

fn independent_report(path: &Path) -> Value {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    assert!(output.status.success(), "{}", path.display());
    serde_json::from_slice(&output.stdout).expect("ffprobe json")
}

fn stream_of<'a>(report: &'a Value, kind: &str) -> Option<&'a Value> {
    report["streams"]
        .as_array()?
        .iter()
        .find(|stream| stream["codec_type"] == kind)
}

fn text_of(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// The independent `moov` before `mdat` among the top-level boxes of an MP4, read from the bytes.
fn moov_before_mdat(bytes: &[u8]) -> bool {
    let mut at = 0usize;
    while at + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize;
        match &bytes[at + 4..at + 8] {
            b"moov" => return true,
            b"mdat" => return false,
            _ => {}
        }
        assert!(size >= 8, "a box of {size} bytes");
        at += size;
    }
    false
}

/// Every field the spec names, compared with what an independent ffprobe says of the same file.
fn assert_probe_equals_independent(facts: &Probe, path: &Path, bytes: &[u8], label: &str) {
    let report = independent_report(path);
    let video = stream_of(&report, "video").expect("a video stream");
    let facts_video = facts.video.as_ref().expect("probe video");
    assert_eq!(facts.kind, MediaKind::Video, "{label}");
    assert_eq!(
        Some(facts.container.clone()),
        text_of(&report["format"]["format_name"]),
        "{label}: container"
    );
    assert_eq!(
        facts_video.codec,
        text_of(&video["codec_name"]),
        "{label}: codec"
    );
    assert_eq!(
        facts_video.profile,
        text_of(&video["profile"]),
        "{label}: profile"
    );
    assert_eq!(
        Some(facts_video.width),
        video["width"].as_u64().map(|v| v as u32),
        "{label}"
    );
    assert_eq!(
        Some(facts_video.height),
        video["height"].as_u64().map(|v| v as u32),
        "{label}"
    );
    assert_eq!(
        facts_video.pix_fmt,
        text_of(&video["pix_fmt"]),
        "{label}: pix_fmt"
    );
    assert_eq!(
        facts_video.primaries,
        text_of(&video["color_primaries"]),
        "{label}: primaries"
    );
    assert_eq!(
        facts_video.transfer,
        text_of(&video["color_transfer"]),
        "{label}: transfer"
    );
    assert_eq!(
        facts_video.matrix,
        text_of(&video["color_space"]),
        "{label}: matrix"
    );
    assert_eq!(
        facts_video.bitrate,
        video["bit_rate"]
            .as_str()
            .and_then(|text| text.parse().ok()),
        "{label}: bitrate"
    );
    let rotation = video["side_data_list"].as_array().and_then(|list| {
        list.iter().find_map(|data| {
            data["rotation"]
                .as_f64()
                .map(|degrees| degrees.round() as i32)
        })
    });
    assert_eq!(facts_video.rotation, rotation, "{label}: rotation");
    assert_eq!(
        facts.audio.as_ref().map(|audio| audio.codec.clone()),
        stream_of(&report, "audio").and_then(|audio| text_of(&audio["codec_name"])),
        "{label}: audio codec"
    );
    let duration = report["format"]["duration"]
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .map(|seconds| (seconds * 1000.0).round() as u64);
    assert_eq!(facts.duration_ms, duration, "{label}: duration");
    if facts.container.contains("mp4") {
        assert_eq!(
            facts.moov_first,
            moov_before_mdat(bytes),
            "{label}: moov_first"
        );
    }
}

#[test]
fn every_field_of_a_video_probe_equals_an_independent_ffprobe_of_the_same_file() {
    let mut runner = MediaRunner::start();
    let cases: [(&str, Make, &str); 5] = [
        (
            "compliant with audio, index first",
            |m| m.av_mp4(true),
            "av.mp4",
        ),
        ("index at the end", |m| m.av_mp4(false), "av.mp4"),
        (
            "rotated by the display matrix",
            Media::rotated_mp4,
            "rotated.mp4",
        ),
        ("HLG", Media::hlg_mp4, "hlg.mp4"),
        ("QuickTime", Media::mov, "clip.mov"),
    ];

    for (label, make, name) in cases {
        let media = Media::new();
        let bytes = make(&media);

        let facts = probed(&mut runner, &bytes);

        assert_probe_equals_independent(&facts, &media.path(name), &bytes, label);
    }
}

#[test]
fn an_hlg_probe_reports_the_hlg_transfer_and_a_rotated_one_the_matrix_angle() {
    let mut runner = MediaRunner::start();
    let media = Media::new();

    let hlg = probed(&mut runner, &media.hlg_mp4());
    let rotated = probed(&mut runner, &media.rotated_mp4());

    assert_eq!(
        hlg.video.expect("video").transfer.as_deref(),
        Some("arib-std-b67")
    );
    assert_eq!(rotated.video.expect("video").rotation, Some(90));
}

const PASSWD_CANARY_LINES: usize = 3;

/// A box header that makes the sniffer take the bytes after it for a video container.
fn behind_a_video_header(text: &str) -> Vec<u8> {
    let mut bytes = b"\0\0\0\x14ftypisom\0\0\0\0isom".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

#[test]
fn playlists_and_concat_files_naming_etc_passwd_are_typed_failures_probed_under_the_whitelists() {
    let passwd = fs::read_to_string("/etc/passwd").expect("read /etc/passwd");
    let canaries: Vec<&str> = passwd.lines().take(PASSWD_CANARY_LINES).collect();
    let playlist = "#EXTM3U\n#EXTINF:1,\nfile:///etc/passwd\n";
    let concat = "ffconcat version 1.0\nfile '/etc/passwd'\n";
    let inputs: [(&str, Vec<u8>, bool); 4] = [
        ("playlist", playlist.as_bytes().to_vec(), false),
        ("concat file", concat.as_bytes().to_vec(), false),
        (
            "playlist behind a video header",
            behind_a_video_header(playlist),
            true,
        ),
        (
            "concat file behind a video header",
            behind_a_video_header(concat),
            true,
        ),
    ];
    let logs = TempDir::new().expect("argv log dir");
    let argv_log = logs.path().join("ffprobe.argv");
    let recorder = format!(
        "for a in \"$@\"; do printf '%s\\n' \"$a\"; done >> {log}; echo '---' >> {log}\nexec {real} \"$@\"",
        log = argv_log.display(),
        real = real_tool("ffprobe").display()
    );
    let (_bin, path) = path_with_scripts(&[("ffprobe", &recorder)]);
    let mut runner = MediaRunner::start_with(&[], &path);
    let mut client = runner.connect();
    let mut ran_ffprobe = 0;

    for (name, bytes, reaches_ffprobe) in inputs {
        let job = runner.job(&bytes);

        let answer = probe(&mut client, &job);

        let shown = format!("{answer:?}");
        assert!(
            matches!(
                answer,
                Response::Error(DecodeError::Unsupported | DecodeError::Corrupt)
            ),
            "{name}: {shown}"
        );
        assert!(canaries.iter().all(|line| !shown.contains(line)), "{name}");
        ran_ffprobe += usize::from(reaches_ffprobe);
    }

    let log = runner.log();
    assert!(
        log.contains("media tool failed"),
        "ffprobe's failure was logged: {log}"
    );
    assert!(
        canaries.iter().all(|line| !log.contains(line)),
        "the runner log names /etc/passwd bytes"
    );
    let recorded = fs::read_to_string(&argv_log).expect("ffprobe was run");
    let calls: Vec<Vec<&str>> = recorded
        .split("---\n")
        .filter(|call| !call.is_empty())
        .map(|call| call.lines().collect())
        .collect();
    assert_eq!(
        calls.len(),
        ran_ffprobe,
        "only the inputs with a video header reach ffprobe"
    );
    for call in calls {
        assert_eq!(
            call,
            [
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
                "-protocol_whitelist",
                "file",
                "-format_whitelist",
                "mov,matroska,avi,mpegts",
                "-codec_whitelist",
                "h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt",
                "-i",
                "/proc/self/fd/9",
            ]
        );
    }
}
