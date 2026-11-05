//! AC-3: strict parsing in both directions: unknown fields and enumeration values, bad UTF-8,
//! malformed JSON, free text in a `Plan`, the wrong role.

mod common;

use common::*;
use media_shared::decoder_protocol::{
    DecodeError, Event, Message, ProtocolViolation, Request, Response, ViolationKind, parse_reply,
    parse_request, write_message,
};
use media_shared::policy::Plan;

#[test]
fn a_known_good_payload_of_each_role_is_accepted() {
    accepted(r#"{"request":{"hello":{"protocol":1}}}"#);
    accepted(&image_result("320", "240", "1"));
    accepted(r#"{"event":{"progress":{"out_time_ms":10,"speed":1.5}}}"#);
    accepted(&render_still("1440"));
}

#[test]
fn an_unknown_field_is_a_violation() {
    let cases = [
        r#"{"request":{"probe":{"job_dir":"/s/j","extra":1}}}"#,
        r#"{"request":{"hello":{"protocol":1,"extra":1}}}"#,
        r#"{"response":{"image_result":{"width":1,"height":1,"bytes":1,"path":"/etc/passwd"}}}"#,
        r#"{"event":{"progress":{"out_time_ms":1,"speed":1.0,"extra":1}}}"#,
    ];
    for case in cases {
        assert_eq!(refused(case.as_bytes()), ViolationKind::Schema, "{case}");
    }
    assert!(matches!(
        refused(br#"{"request":"cancel","extra":1}"#),
        ViolationKind::Json | ViolationKind::Schema
    ));
}

#[test]
fn an_unknown_enumeration_value_is_a_violation() {
    let unknown_kind = probe_reply(&video("1920", "null", "null", "null"), "1")
        .replace("video\",\"container", "audio\",\"container");
    let cases = [
        unknown_kind.as_str(),
        r#"{"request":{"launch":{"protocol":1}}}"#,
        r#"{"response":"finished"}"#,
        r#"{"response":{"error":{"exploded":null}}}"#,
        r#"{"response":{"error":{"limit_exceeded":{"limit":"cpu"}}}}"#,
        r#"{"event":"heartbeat"}"#,
        r#"{"command":"cancel"}"#,
    ];
    for case in cases {
        assert_eq!(refused(case.as_bytes()), ViolationKind::Schema, "{case}");
    }
}

#[test]
fn invalid_utf8_is_a_violation() {
    let mut payload = br#"{"request":{"probe":{"job_dir":""#.to_vec();
    payload.extend_from_slice(&[0xff, 0xfe, 0xc0, 0xaf]);
    payload.extend_from_slice(br#""}}}"#);

    assert_eq!(refused(&payload), ViolationKind::NotUtf8);
    assert_eq!(refused(&[0xff]), ViolationKind::NotUtf8);
}

#[test]
fn malformed_json_is_a_violation() {
    let cases: [&[u8]; 8] = [
        b"",
        b"{",
        br#"{"request":"#,
        br#"{"request":"cancel"} {"request":"cancel"}"#,
        br#"{"request":"cancel"}x"#,
        b"null",
        b"\x00",
        br#"{'request':'cancel'}"#,
    ];
    for case in cases {
        assert_eq!(
            refused(case),
            ViolationKind::Json,
            "{}",
            String::from_utf8_lossy(case)
        );
    }
}

#[test]
fn deep_nesting_is_a_violation_not_a_crash() {
    let nested = "[".repeat(60_000);
    let wrapped = format!(r#"{{"request":{{"probe":{{"job_dir":{nested}"#);

    refused(nested.as_bytes());
    refused(wrapped.as_bytes());
}

#[test]
fn a_duplicate_field_is_a_violation() {
    let duplicate = r#"{"request":{"hello":{"protocol":1,"protocol":2}}}"#;
    let two_roles = r#"{"request":"cancel","request":"cancel"}"#;

    assert_eq!(refused(duplicate.as_bytes()), ViolationKind::Schema);
    refused(two_roles.as_bytes());
}

#[test]
fn a_missing_field_is_a_violation() {
    assert_eq!(
        refused(br#"{"request":{"hello":{}}}"#),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(br#"{"response":{"image_result":{"width":1,"height":1}}}"#),
        ViolationKind::Schema
    );
}

#[test]
fn free_text_in_a_plan_is_a_violation() {
    let cases = [
        transcode(r#""remux -vf movie=/etc/passwd""#),
        transcode(
            r#"{"transcode":{"encoder":"libx264 -vf x","scale_to_short_edge":1080,"tone_map":true,"audio":"aac","faststart":true}}"#,
        ),
        transcode(
            r#"{"transcode":{"encoder":"cpu","scale_to_short_edge":"1080","tone_map":true,"audio":"aac","faststart":true}}"#,
        ),
        transcode(
            r#"{"transcode":{"encoder":"cpu","scale_to_short_edge":1080,"tone_map":true,"audio":"aac","faststart":true,"extra_args":"-f lavfi"}}"#,
        ),
        transcode(
            r#"{"transcode":{"encoder":"cpu","scale_to_short_edge":1080,"tone_map":"yes","audio":"aac","faststart":true}}"#,
        ),
        transcode(r#"{"remux":{"args":"-i x"}}"#),
    ];
    for case in &cases {
        assert_eq!(refused(case.as_bytes()), ViolationKind::Schema, "{case}");
    }
}

#[test]
fn a_valid_plan_is_accepted() {
    let plan = r#"{"transcode":{"encoder":"nvenc","scale_to_short_edge":1080,"tone_map":true,"audio":"aac","faststart":true}}"#;

    let message = accepted(&transcode(plan));

    assert_eq!(
        message,
        Message::Request(Request::Transcode {
            job_dir: "/s/j".to_owned(),
            plan: transcode_plan(),
            limits: limits(),
        })
    );
    assert!(matches!(
        accepted(&transcode(r#""remux""#)),
        Message::Request(Request::Transcode {
            plan: Plan::Remux,
            ..
        })
    ));
}

#[test]
fn the_wrong_role_is_a_violation_in_each_direction() {
    let request = br#"{"request":"cancel"}"#;
    let response = br#"{"response":"cancelled"}"#;
    let event = br#"{"event":"alive"}"#;

    assert!(parse_request(request).is_ok());
    assert!(parse_reply(response).is_ok());
    assert!(parse_reply(event).is_ok());
    for payload in [&response[..], &event[..]] {
        assert_eq!(
            parse_request(payload),
            Err(ProtocolViolation(ViolationKind::WrongDirection))
        );
    }
    assert_eq!(
        parse_reply(request).map(|_| ()),
        Err(ProtocolViolation(ViolationKind::WrongDirection))
    );
}

#[test]
fn a_sender_cannot_emit_what_the_parser_refuses() {
    let nan = Message::Event(Event::Progress {
        out_time_ms: 1,
        speed: f64::NAN,
    });
    let wide = Message::Response(Response::ImageResult(
        media_shared::decoder_protocol::ImageResult {
            width: u32::MAX,
            height: 1,
            bytes: 1,
        },
    ));
    let failed = Message::Response(Response::Error(DecodeError::EncoderFailed { exit_code: 0 }));
    let mut wire = Vec::new();

    for message in [nan, wide, failed] {
        assert!(write_message(&mut wire, &message).is_err(), "{message:?}");
    }

    assert!(wire.is_empty());
}

#[test]
fn a_violation_maps_to_the_protocol_violation_error() {
    let error: DecodeError = ProtocolViolation(ViolationKind::Oversize).into();

    assert_eq!(error, DecodeError::ProtocolViolation);
}
