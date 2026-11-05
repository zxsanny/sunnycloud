//! AC-2: decoder messages reject unknown fields (10 section 3: the decoder is untrusted).
//!
//! Every object of every sample message is mutated in turn with one extra field; each mutant
//! must fail to parse, so a nested struct that forgot `deny_unknown_fields` is found.

mod common;

use common::*;
use media_shared::decoder_protocol::{Request, Response, TranscodeResult};
use media_shared::policy::{Decision, Plan, Probe};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::fmt::Debug;

fn mutants(value: &Value) -> Vec<Value> {
    match value {
        Value::Object(map) => {
            let mut out = Vec::new();
            let mut extra = map.clone();
            extra.insert("extra_field".to_owned(), json!(1));
            out.push(Value::Object(extra));
            for (key, child) in map {
                for mutant in mutants(child) {
                    let mut copy: Map<String, Value> = map.clone();
                    copy.insert(key.clone(), mutant);
                    out.push(Value::Object(copy));
                }
            }
            out
        }
        Value::Array(items) => {
            let mut out = Vec::new();
            for (index, child) in items.iter().enumerate() {
                for mutant in mutants(child) {
                    let mut copy = items.clone();
                    copy[index] = mutant;
                    out.push(Value::Array(copy));
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Returns the number of mutants checked.
fn assert_strict<T: Serialize + DeserializeOwned + PartialEq + Debug>(sample: &T) -> usize {
    let value = serde_json::to_value(sample).expect("serialise");
    assert_eq!(
        &serde_json::from_value::<T>(value.clone()).expect("original parses"),
        sample
    );
    let all = mutants(&value);
    for mutant in &all {
        assert!(
            serde_json::from_value::<T>(mutant.clone()).is_err(),
            "accepted {mutant} for {value}"
        );
    }
    all.len()
}

#[test]
fn every_request_rejects_an_extra_field() {
    for request in requests() {
        let checked = assert_strict(&request);
        if !matches!(request, Request::Cancel) {
            assert!(checked >= 1, "no mutant for {request:?}");
        }
    }
}

#[test]
fn every_response_rejects_an_extra_field() {
    for response in responses() {
        assert_strict(&response);
    }
}

#[test]
fn every_event_rejects_an_extra_field() {
    for event in events() {
        assert_strict(&event);
    }
}

#[test]
fn every_decode_error_rejects_an_extra_field() {
    for error in decode_errors() {
        assert_strict(&error);
    }
}

#[test]
fn shared_policy_types_on_the_socket_reject_an_extra_field() {
    assert!(assert_strict(&video_probe()) >= 3);
    assert!(assert_strict(&photo_probe()) >= 2);
    for plan in plans() {
        assert_strict(&plan);
    }
    assert_strict(&Decision::Transcode(transcode_plan()));
    assert_strict(&capabilities());
    assert_strict(&limits());
}

#[test]
fn a_reply_with_one_extra_field_fails_to_parse() {
    let reply = r#"{"image_result":{"width":320,"height":240,"bytes":18000,"path":"/etc/passwd"}}"#;

    let parsed = serde_json::from_str::<Response>(reply);

    assert!(parsed.is_err());
}

#[test]
fn an_extra_field_inside_a_nested_probe_fails_to_parse() {
    let mut reply = serde_json::to_value(Response::TranscodeResult(TranscodeResult {
        probe_of_output: video_probe(),
        bytes: 1,
    }))
    .expect("serialise");
    reply["transcode_result"]["probe_of_output"]["video"]["codec_args"] = json!("-vf x");

    assert!(serde_json::from_value::<Response>(reply).is_err());
}

#[test]
fn an_extra_field_in_a_request_limits_fails_to_parse() {
    let request = r#"{"transcode":{"job_dir":"/s/j","plan":"remux","limits":{"cpu":1,"memory_bytes":1,"wall":1,"stall_timeout":1,"max_output_bytes":1,"nice":-20}}}"#;

    assert!(serde_json::from_str::<Request>(request).is_err());
}

#[test]
fn a_string_where_a_plan_field_is_an_enumeration_fails_to_parse() {
    let plan = r#"{"transcode":{"encoder":"libx264 -vf movie=/etc/passwd","scale_to_short_edge":1080,"tone_map":true,"audio":"aac","faststart":true}}"#;

    assert!(serde_json::from_str::<Plan>(plan).is_err());
}

#[test]
fn an_unknown_enumeration_value_fails_to_parse() {
    assert!(serde_json::from_str::<Request>(r#"{"launch":{"protocol":1}}"#).is_err());
    assert!(serde_json::from_str::<Response>(r#""finished""#).is_err());
    assert!(
        serde_json::from_str::<Probe>(&json_of(&json!({
            "kind": "audio", "container": "x", "moov_first": true,
            "video": null, "audio": null, "duration_ms": null
        })))
        .is_err()
    );
}

#[test]
fn a_missing_required_field_fails_to_parse() {
    assert!(serde_json::from_str::<Request>(r#"{"hello":{}}"#).is_err());
    assert!(
        serde_json::from_str::<Response>(r#"{"image_result":{"width":1,"height":1}}"#).is_err()
    );
}

#[test]
fn job_dtos_of_the_worker_routes_are_not_strict() {
    let lease = r#"{"lease_ms":5,"added_later":true}"#;

    let parsed = serde_json::from_str::<media_shared::types::LeaseExtended>(lease);

    assert_eq!(parsed.expect("lenient").lease_ms, 5);
}
