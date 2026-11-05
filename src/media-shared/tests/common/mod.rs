//! One value of every variant of every type, shared by the round-trip, wire-name and
//! strict-parsing tests.

// Each test binary compiles this module and uses a different subset of it.
#![allow(dead_code)]

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, Event, ImageResult, LimitKind, Limits, Message, ProtocolViolation,
    Request, Response, SelfCheckItem, SelfCheckReport, TranscodeResult, ViolationKind,
    parse_message,
};
use media_shared::policy::{
    AudioPlan, AudioStream, Decision, Encoder, MediaKind, OutputSpec, Params, Plan, Probe,
    VideoStream, X264Preset,
};
use media_shared::types::{
    ClaimedJob, DerivedKind, Disposition, FailResult, FailureReport, JobKind, Lane, LeaseExtended,
    OutputMeta, SkipOutcome, WorkerStatus,
};
use sc_ids::FileHash;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use uuid::Uuid;

pub fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let json = serde_json::to_string(value).expect("serialise");
    let parsed: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse {json}: {e}"));
    assert_eq!(&parsed, value, "round trip of {json}");
}

pub fn json_of<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialise")
}

pub fn file_hash() -> FileHash {
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::try_from(index * 8 + 1).expect("fits");
    }
    FileHash::from_bytes(bytes)
}

pub fn claimed_job() -> ClaimedJob {
    ClaimedJob {
        job_id: Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
        kind: JobKind::Transcode,
        content_hash: file_hash(),
        params_version: 1,
        attempt: 2,
        lease_ms: 90_000,
    }
}

pub const LANES: [Lane; 2] = [Lane::Image, Lane::Video];
pub const JOB_KINDS: [JobKind; 3] = [JobKind::Stills, JobKind::Poster, JobKind::Transcode];
pub const DERIVED_KINDS: [DerivedKind; 3] = [
    DerivedKind::Thumb,
    DerivedKind::Preview,
    DerivedKind::Transcode,
];
pub const SKIP_OUTCOMES: [SkipOutcome; 2] = [SkipOutcome::SkippedCompliant, SkipOutcome::Obsolete];
pub const DISPOSITIONS: [Disposition; 2] = [Disposition::Retry, Disposition::Permanent];
pub const FAIL_RESULTS: [FailResult; 2] = [FailResult::Requeued, FailResult::Dead];

pub fn output_meta() -> OutputMeta {
    OutputMeta {
        media_type: "video/mp4".to_owned(),
        width: Some(1920),
        height: Some(1080),
        duration_ms: Some(61_500),
    }
}

pub fn lease_extended() -> LeaseExtended {
    LeaseExtended { lease_ms: 90_000 }
}

pub fn failure_report() -> FailureReport {
    FailureReport {
        code: "corrupt".to_owned(),
        disposition: Disposition::Permanent,
    }
}

pub fn worker_status() -> WorkerStatus {
    WorkerStatus {
        decoder_ok: true,
        image_age_days: 3,
    }
}

pub fn video_stream() -> VideoStream {
    VideoStream {
        codec: Some("h264".to_owned()),
        profile: Some("High".to_owned()),
        width: 1920,
        height: 1080,
        rotation: Some(-90),
        bitrate: Some(8_000_000),
        pix_fmt: Some("yuv420p".to_owned()),
        primaries: Some("bt709".to_owned()),
        transfer: Some("bt709".to_owned()),
        matrix: Some("bt709".to_owned()),
    }
}

pub fn video_probe() -> Probe {
    Probe {
        kind: MediaKind::Video,
        container: "mov,mp4,m4a,3gp,3g2,mj2".to_owned(),
        moov_first: true,
        video: Some(video_stream()),
        audio: Some(AudioStream {
            codec: "aac".to_owned(),
        }),
        duration_ms: Some(61_500),
    }
}

pub fn photo_probe() -> Probe {
    Probe {
        kind: MediaKind::Photo,
        container: "jpeg".to_owned(),
        moov_first: false,
        video: Some(VideoStream {
            codec: None,
            profile: None,
            width: 4032,
            height: 3024,
            rotation: None,
            bitrate: None,
            pix_fmt: None,
            primaries: None,
            transfer: None,
            matrix: None,
        }),
        audio: None,
        duration_ms: None,
    }
}

pub fn transcode_plan() -> Plan {
    Plan::Transcode {
        encoder: Encoder::Nvenc,
        scale_to_short_edge: 1080,
        tone_map: true,
        audio: AudioPlan::Aac,
        faststart: true,
    }
}

pub fn plans() -> Vec<Plan> {
    vec![
        Plan::Remux,
        transcode_plan(),
        Plan::Transcode {
            encoder: Encoder::Cpu,
            scale_to_short_edge: 720,
            tone_map: false,
            audio: AudioPlan::None,
            faststart: false,
        },
    ]
}

pub fn decisions() -> Vec<Decision> {
    vec![
        Decision::SkipCompliant,
        Decision::Remux,
        Decision::Transcode(transcode_plan()),
    ]
}

pub fn params() -> Params {
    Params {
        thumb_edge: 320,
        preview_edge: 1440,
        webp_quality: 80,
        crf: 23,
        preset: X264Preset::Veryfast,
    }
}

pub const PRESETS: [X264Preset; 10] = [
    X264Preset::Ultrafast,
    X264Preset::Superfast,
    X264Preset::Veryfast,
    X264Preset::Faster,
    X264Preset::Fast,
    X264Preset::Medium,
    X264Preset::Slow,
    X264Preset::Slower,
    X264Preset::Veryslow,
    X264Preset::Placebo,
];

pub fn output_spec() -> OutputSpec {
    OutputSpec {
        kind: DerivedKind::Thumb,
        media_types: vec!["image/webp".to_owned(), "image/jpeg".to_owned()],
        max_edge: Some(320),
        max_bytes: 5 * 1024 * 1024,
    }
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        protocol: 1,
        vips: "8.18.0".to_owned(),
        ffmpeg: "8.0".to_owned(),
        libheif: "1.20.2".to_owned(),
        build_timestamp: 1_793_000_000,
        nvenc_probe_ok: false,
        nvenc_sessions: 0,
    }
}

pub fn limits() -> Limits {
    Limits {
        cpu: 4,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        wall: 2_400_000,
        stall_timeout: 90_000,
        max_output_bytes: 10 * 1024 * 1024 * 1024,
    }
}

pub fn requests() -> Vec<Request> {
    vec![
        Request::Hello { protocol: 1 },
        Request::SelfCheck {
            forbidden_targets: vec!["api:8080".to_owned(), "db:5432".to_owned()],
        },
        Request::Probe {
            job_dir: "/scratch/job-1".to_owned(),
        },
        Request::RenderStill {
            job_dir: "/scratch/job-1".to_owned(),
            max_edge: 1440,
            poster_at_ms: Some(1_000),
            limits: limits(),
        },
        Request::RenderStill {
            job_dir: "/scratch/job-1".to_owned(),
            max_edge: 320,
            poster_at_ms: None,
            limits: limits(),
        },
        Request::Transcode {
            job_dir: "/scratch/job-2".to_owned(),
            plan: transcode_plan(),
            limits: limits(),
        },
        Request::Transcode {
            job_dir: "/scratch/job-2".to_owned(),
            plan: Plan::Remux,
            limits: limits(),
        },
        Request::Cancel,
    ]
}

pub const LIMIT_KINDS: [LimitKind; 4] = [
    LimitKind::Wall,
    LimitKind::Memory,
    LimitKind::Pixels,
    LimitKind::Output,
];

pub fn decode_errors() -> Vec<DecodeError> {
    let mut errors = vec![
        DecodeError::ProtocolMismatch,
        DecodeError::ProtocolViolation,
        DecodeError::Unsupported,
        DecodeError::Corrupt,
        DecodeError::Busy,
        DecodeError::Stalled,
        DecodeError::EncoderFailed { exit_code: 187 },
        DecodeError::OutputInvalid,
    ];
    errors.extend(LIMIT_KINDS.map(|limit| DecodeError::LimitExceeded { limit }));
    errors
}

pub fn self_check_report() -> SelfCheckReport {
    SelfCheckReport(vec![
        SelfCheckItem {
            name: "network".to_owned(),
            ok: true,
        },
        SelfCheckItem {
            name: "root_filesystem_read_only".to_owned(),
            ok: false,
        },
    ])
}

pub fn responses() -> Vec<Response> {
    let mut responses = vec![
        Response::Capabilities(capabilities()),
        Response::SelfCheck(self_check_report()),
        Response::SelfCheck(SelfCheckReport(Vec::new())),
        Response::Probe(video_probe()),
        Response::Probe(photo_probe()),
        Response::ImageResult(ImageResult {
            width: 320,
            height: 240,
            bytes: 18_000,
        }),
        Response::TranscodeResult(TranscodeResult {
            probe_of_output: video_probe(),
            bytes: 40_000_000,
        }),
        Response::Cancelled,
    ];
    responses.extend(decode_errors().into_iter().map(Response::Error));
    responses
}

pub fn events() -> Vec<Event> {
    vec![
        Event::Alive,
        Event::Progress {
            out_time_ms: 12_500,
            speed: 1.5,
        },
    ]
}

/// One message of every role and variant, as it travels in a frame.
pub fn messages() -> Vec<Message> {
    let mut all: Vec<Message> = requests().into_iter().map(Message::Request).collect();
    all.extend(responses().into_iter().map(Message::Response));
    all.extend(events().into_iter().map(Message::Event));
    all
}

/// A frame with a correct big-endian prefix around `payload`.
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(payload.len()).expect("payload fits the prefix");
    let mut bytes = length.to_be_bytes().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

/// The `limits` object of a request, as JSON text.
pub const LIMITS_JSON: &str = r#"{"cpu":4,"memory_bytes":4294967296,"wall":2400000,"stall_timeout":90000,"max_output_bytes":10737418240}"#;

pub fn refused(payload: &[u8]) -> ViolationKind {
    match parse_message(payload) {
        Err(ProtocolViolation(kind)) => kind,
        Ok(message) => panic!(
            "accepted {message:?} for {}",
            String::from_utf8_lossy(payload)
        ),
    }
}

pub fn out_of_range(field: &'static str) -> ViolationKind {
    ViolationKind::OutOfRange(field)
}

pub fn render_still(max_edge: &str) -> String {
    format!(
        r#"{{"request":{{"render_still":{{"job_dir":"/s/j","max_edge":{max_edge},"poster_at_ms":null,"limits":{LIMITS_JSON}}}}}}}"#
    )
}

pub fn image_result(width: &str, height: &str, bytes: &str) -> String {
    format!(
        r#"{{"response":{{"image_result":{{"width":{width},"height":{height},"bytes":{bytes}}}}}}}"#
    )
}

pub fn probe_reply(video: &str, duration_ms: &str) -> String {
    format!(
        r#"{{"response":{{"probe":{{"kind":"video","container":"mp4","moov_first":true,"video":{video},"audio":null,"duration_ms":{duration_ms}}}}}}}"#
    )
}

pub fn video(width: &str, rotation: &str, bitrate: &str, codec: &str) -> String {
    format!(
        r#"{{"codec":{codec},"profile":null,"width":{width},"height":1080,"rotation":{rotation},"bitrate":{bitrate},"pix_fmt":null,"primaries":null,"transfer":null,"matrix":null}}"#
    )
}

pub fn transcode(plan: &str) -> String {
    format!(
        r#"{{"request":{{"transcode":{{"job_dir":"/s/j","plan":{plan},"limits":{LIMITS_JSON}}}}}}}"#
    )
}

pub fn accepted(payload: &str) -> Message {
    parse_message(payload.as_bytes()).unwrap_or_else(|v| panic!("refused {payload}: {v}"))
}

pub fn limits_with(field: &str, value: &str) -> String {
    let mut object: serde_json::Value = serde_json::from_str(LIMITS_JSON).expect("json");
    object[field] = serde_json::from_str(value).expect("json");
    object.to_string()
}
