//! AC-3: the serialised names of `types` are the ones the worker routes of 08 section 5 use,
//! and the policy and protocol enums keep the names the decoder contract lists.

mod common;

use common::*;
use media_shared::decoder_protocol::{DecodeError, Event, LimitKind, Request, Response};
use media_shared::policy::{AudioPlan, Decision, Encoder, MediaKind, Plan};
use media_shared::types::{ClaimedJob, FailureReport, JobKind};

#[test]
fn route_enums_use_the_worker_route_names() {
    assert_eq!(json_of(&LANES), r#"["image","video"]"#);
    assert_eq!(json_of(&JOB_KINDS), r#"["stills","poster","transcode"]"#);
    assert_eq!(
        json_of(&DERIVED_KINDS),
        r#"["thumb","preview","transcode"]"#
    );
    assert_eq!(
        json_of(&SKIP_OUTCOMES),
        r#"["skipped_compliant","obsolete"]"#
    );
    assert_eq!(json_of(&DISPOSITIONS), r#"["retry","permanent"]"#);
    assert_eq!(json_of(&FAIL_RESULTS), r#"["requeued","dead"]"#);
}

#[test]
fn the_four_named_values_read_as_the_routes_spell_them() {
    use media_shared::types::{Disposition, FailResult, Lane, SkipOutcome};

    assert_eq!(json_of(&Lane::Image), r#""image""#);
    assert_eq!(
        json_of(&SkipOutcome::SkippedCompliant),
        r#""skipped_compliant""#
    );
    assert_eq!(json_of(&Disposition::Permanent), r#""permanent""#);
    assert_eq!(json_of(&FailResult::Dead), r#""dead""#);
}

#[test]
fn claimed_job_has_the_route_field_names_and_a_hex_hash() {
    let json = json_of(&claimed_job());

    assert_eq!(
        json,
        concat!(
            r#"{"job_id":"01234567-89ab-cdef-0123-456789abcdef","kind":"transcode","#,
            r#""content_hash":"0109111921293139414951596169717981899199a1a9b1b9c1c9d1d9e1e9f1f9","#,
            r#""params_version":1,"attempt":2,"lease_ms":90000}"#
        )
    );
}

#[test]
fn content_hash_is_the_display_form_of_file_hash() {
    let job = claimed_job();

    let json = json_of(&job);

    assert!(json.contains(&format!(r#""content_hash":"{}""#, job.content_hash)));
}

#[test]
fn malformed_content_hash_is_rejected() {
    let valid = "0109111921293139414951596169717981899199a1a9b1b9c1c9d1d9e1e9f1f9";
    let bad = [
        valid.to_uppercase(),
        valid[..62].to_owned(),
        format!("{valid}00"),
        format!("{}g", &valid[..63]),
        String::new(),
        "AAAA".to_owned(),
    ];

    for hash in bad {
        let json = format!(
            r#"{{"job_id":"01234567-89ab-cdef-0123-456789abcdef","kind":"stills","content_hash":"{hash}","params_version":1,"attempt":0,"lease_ms":1}}"#
        );
        assert!(
            serde_json::from_str::<ClaimedJob>(&json).is_err(),
            "accepted content_hash {hash:?}"
        );
    }
}

#[test]
fn content_hash_must_be_a_string() {
    let json = r#"{"job_id":"01234567-89ab-cdef-0123-456789abcdef","kind":"stills","content_hash":[1,2,3],"params_version":1,"attempt":0,"lease_ms":1}"#;

    assert!(serde_json::from_str::<ClaimedJob>(json).is_err());
}

#[test]
fn unknown_job_kind_or_lane_is_rejected() {
    assert!(serde_json::from_str::<JobKind>(r#""Stills""#).is_err());
    assert!(serde_json::from_str::<JobKind>(r#""thumbnail""#).is_err());
    assert!(serde_json::from_str::<media_shared::types::Lane>(r#""audio""#).is_err());
}

#[test]
fn body_structs_use_the_route_field_names() {
    assert_eq!(json_of(&lease_extended()), r#"{"lease_ms":90000}"#);
    assert_eq!(
        json_of(&worker_status()),
        r#"{"decoder_ok":true,"image_age_days":3}"#
    );
    assert_eq!(
        json_of(&failure_report()),
        r#"{"code":"corrupt","disposition":"permanent"}"#
    );
    assert_eq!(
        json_of(&output_meta()),
        r#"{"media_type":"video/mp4","width":1920,"height":1080,"duration_ms":61500}"#
    );
    let parsed: FailureReport =
        serde_json::from_str(r#"{"code":"x","disposition":"retry"}"#).expect("route body parses");
    assert_eq!(parsed.disposition, media_shared::types::Disposition::Retry);
}

#[test]
fn policy_enums_use_snake_case_names() {
    assert_eq!(
        json_of(&[MediaKind::Photo, MediaKind::Video]),
        r#"["photo","video"]"#
    );
    assert_eq!(
        json_of(&[Encoder::Cpu, Encoder::Nvenc]),
        r#"["cpu","nvenc"]"#
    );
    assert_eq!(
        json_of(&[AudioPlan::Aac, AudioPlan::None]),
        r#"["aac","none"]"#
    );
    assert_eq!(json_of(&Plan::Remux), r#""remux""#);
    assert_eq!(
        json_of(&transcode_plan()),
        r#"{"transcode":{"encoder":"nvenc","scale_to_short_edge":1080,"tone_map":true,"audio":"aac","faststart":true}}"#
    );
    assert_eq!(json_of(&Decision::SkipCompliant), r#""skip_compliant""#);
    assert_eq!(json_of(&Decision::Remux), r#""remux""#);
}

#[test]
fn decoder_message_kinds_use_the_protocol_names() {
    let names: Vec<String> = requests()
        .iter()
        .map(|request| first_key(&json_of(request)))
        .collect();
    assert_eq!(
        names,
        [
            "hello",
            "self_check",
            "probe",
            "render_still",
            "render_still",
            "transcode",
            "transcode",
            "cancel"
        ]
    );
    assert_eq!(json_of(&Request::Cancel), r#""cancel""#);
    assert_eq!(json_of(&Response::Cancelled), r#""cancelled""#);
    assert_eq!(json_of(&Event::Alive), r#""alive""#);
    assert_eq!(
        json_of(&Event::Progress {
            out_time_ms: 5,
            speed: 1.5
        }),
        r#"{"progress":{"out_time_ms":5,"speed":1.5}}"#
    );
}

#[test]
fn decode_errors_use_the_protocol_names() {
    assert_eq!(
        json_of(&decode_errors()),
        concat!(
            r#"["protocol_mismatch","protocol_violation","unsupported","corrupt","busy","stalled","#,
            r#"{"encoder_failed":{"exit_code":187}},"output_invalid","#,
            r#"{"limit_exceeded":{"limit":"wall"}},{"limit_exceeded":{"limit":"memory"}},"#,
            r#"{"limit_exceeded":{"limit":"pixels"}},{"limit_exceeded":{"limit":"output"}}]"#
        )
    );
    assert_eq!(
        json_of(&LIMIT_KINDS),
        r#"["wall","memory","pixels","output"]"#
    );
    assert_eq!(
        serde_json::from_str::<DecodeError>(r#""busy""#).expect("unit variant"),
        DecodeError::Busy
    );
    assert_eq!(
        serde_json::from_str::<LimitKind>(r#""pixels""#).expect("unit variant"),
        LimitKind::Pixels
    );
}

#[test]
fn render_still_carries_the_documented_fields() {
    let json = json_of(&requests()[3]);

    assert_eq!(
        json,
        concat!(
            r#"{"render_still":{"job_dir":"/scratch/job-1","max_edge":1440,"poster_at_ms":1000,"#,
            r#""limits":{"cpu":4,"memory_bytes":4294967296,"wall":2400000,"stall_timeout":90000,"#,
            r#""max_output_bytes":10737418240}}}"#
        )
    );
}

fn first_key(json: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(json).expect("json");
    match value {
        serde_json::Value::String(name) => name,
        serde_json::Value::Object(map) => map.keys().next().expect("one key").clone(),
        other => panic!("unexpected {other}"),
    }
}
