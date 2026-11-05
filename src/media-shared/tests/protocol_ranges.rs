//! AC-4, AC-5: the range table of the contract, field by field, and the limits that are
//! accepted above the runner's ceilings.

mod common;

use common::*;
use media_shared::decoder_protocol::ranges::{MAX_DURATION_MS, MAX_EDGE};
use media_shared::decoder_protocol::{Message, Request, ViolationKind};

#[test]
fn max_edge_at_and_beyond_the_edges_is_a_violation() {
    assert_eq!(
        refused(render_still("-1").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(render_still("0").as_bytes()),
        out_of_range("max_edge")
    );
    assert_eq!(
        refused(render_still("100000").as_bytes()),
        out_of_range("max_edge")
    );
    assert_eq!(
        refused(render_still(&(MAX_EDGE + 1).to_string()).as_bytes()),
        out_of_range("max_edge")
    );
    assert_eq!(
        refused(render_still("4294967296").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(render_still("1.5").as_bytes()),
        ViolationKind::Schema
    );
    accepted(&render_still("1"));
    accepted(&render_still(&MAX_EDGE.to_string()));
}

#[test]
fn a_reply_width_of_zero_or_u32_max_is_a_violation() {
    assert_eq!(
        refused(image_result("0", "240", "1").as_bytes()),
        out_of_range("width")
    );
    assert_eq!(
        refused(image_result("4294967295", "240", "1").as_bytes()),
        out_of_range("width")
    );
    assert_eq!(
        refused(image_result("320", "0", "1").as_bytes()),
        out_of_range("height")
    );
    assert_eq!(
        refused(image_result("-1", "240", "1").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(image_result("320", "240", "0").as_bytes()),
        out_of_range("bytes")
    );
    assert_eq!(
        refused(probe_reply(&video("0", "null", "null", "null"), "1").as_bytes()),
        out_of_range("video.width")
    );
    assert_eq!(
        refused(probe_reply(&video("4294967295", "null", "null", "null"), "1").as_bytes()),
        out_of_range("video.width")
    );
    accepted(&image_result("16383", "1", "1"));
}

#[test]
fn a_negative_or_oversized_duration_is_a_violation() {
    let ok = video("1920", "null", "null", "null");

    assert_eq!(
        refused(probe_reply(&ok, "-1").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(probe_reply(&ok, "-0.5").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(probe_reply(&ok, &(MAX_DURATION_MS + 1).to_string()).as_bytes()),
        out_of_range("duration_ms")
    );
    accepted(&probe_reply(&ok, &MAX_DURATION_MS.to_string()));
}

#[test]
fn the_other_probe_fields_are_range_checked() {
    let case = |video: String| refused(probe_reply(&video, "1").as_bytes());

    assert_eq!(
        case(video("1920", "361", "null", "null")),
        out_of_range("video.rotation")
    );
    assert_eq!(
        case(video("1920", "-361", "null", "null")),
        out_of_range("video.rotation")
    );
    assert_eq!(
        case(video("1920", "null", "1000000000001", "null")),
        out_of_range("video.bitrate")
    );
    assert_eq!(
        case(video("1920", "null", "null", r#""""#)),
        out_of_range("video.codec")
    );
    assert_eq!(
        case(video("1920", "null", "null", r#""h264\nHTTP/1.1 200""#)),
        out_of_range("video.codec")
    );
    assert_eq!(
        case(video("1920", "null", "null", r#""hä264""#)),
        out_of_range("video.codec")
    );
    accepted(&probe_reply(&video("1920", "-360", "0", r#""h264""#), "0"));
}

#[test]
fn the_progress_event_is_range_checked() {
    let progress = |out: &str, speed: &str| {
        format!(r#"{{"event":{{"progress":{{"out_time_ms":{out},"speed":{speed}}}}}}}"#)
    };

    assert_eq!(
        refused(progress("1", "-0.1").as_bytes()),
        out_of_range("speed")
    );
    assert_eq!(
        refused(progress("1", "1000.5").as_bytes()),
        out_of_range("speed")
    );
    assert_eq!(
        refused(progress("1", "1e999").as_bytes()),
        ViolationKind::Json
    );
    assert_eq!(
        refused(progress("1", "null").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(progress("-1", "1.0").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(
        refused(progress(&(MAX_DURATION_MS + 1).to_string(), "1.0").as_bytes()),
        out_of_range("out_time_ms")
    );
    accepted(&progress("0", "0.0"));
    accepted(&progress("1", "1000"));
}

#[test]
fn the_request_text_and_count_fields_are_range_checked() {
    let probe = |dir: &str| format!(r#"{{"request":{{"probe":{{"job_dir":{dir}}}}}}}"#);
    let targets = |n: usize| {
        let list = vec![r#""api:8080""#; n].join(",");
        format!(r#"{{"request":{{"self_check":{{"forbidden_targets":[{list}]}}}}}}"#)
    };
    let long_dir = format!("\"/{}\"", "d".repeat(4096));

    assert_eq!(refused(probe(r#""""#).as_bytes()), out_of_range("job_dir"));
    assert_eq!(
        refused(probe(r#""/s/\u0000/j""#).as_bytes()),
        out_of_range("job_dir")
    );
    assert_eq!(
        refused(probe(&long_dir).as_bytes()),
        out_of_range("job_dir")
    );
    assert_eq!(
        refused(targets(65).as_bytes()),
        out_of_range("forbidden_targets")
    );
    assert_eq!(
        refused(br#"{"request":{"self_check":{"forbidden_targets":["api 8080"]}}}"#),
        out_of_range("forbidden_targets[]")
    );
    assert_eq!(
        refused(br#"{"request":{"self_check":{"forbidden_targets":[""]}}}"#),
        out_of_range("forbidden_targets[]")
    );
    accepted(&probe(r#""/s/j""#));
    accepted(&targets(64));
    accepted(&targets(0));
}

#[test]
fn the_request_numbers_are_range_checked() {
    let still = |poster: &str| {
        format!(
            r#"{{"request":{{"render_still":{{"job_dir":"/s/j","max_edge":320,"poster_at_ms":{poster},"limits":{LIMITS_JSON}}}}}}}"#
        )
    };
    let plan = |edge: &str| {
        transcode(&format!(
            r#"{{"transcode":{{"encoder":"cpu","scale_to_short_edge":{edge},"tone_map":false,"audio":"none","faststart":true}}}}"#
        ))
    };

    assert_eq!(
        refused(still(&(MAX_DURATION_MS + 1).to_string()).as_bytes()),
        out_of_range("poster_at_ms")
    );
    assert_eq!(refused(still("-1").as_bytes()), ViolationKind::Schema);
    assert_eq!(
        refused(plan("0").as_bytes()),
        out_of_range("plan.scale_to_short_edge")
    );
    assert_eq!(
        refused(plan("16384").as_bytes()),
        out_of_range("plan.scale_to_short_edge")
    );
    assert_eq!(refused(plan("65536").as_bytes()), ViolationKind::Schema);
    accepted(&still("0"));
    accepted(&plan("720"));
}

#[test]
fn a_zero_limit_is_a_violation() {
    let fields = [
        ("cpu", "limits.cpu"),
        ("memory_bytes", "limits.memory_bytes"),
        ("wall", "limits.wall"),
        ("stall_timeout", "limits.stall_timeout"),
        ("max_output_bytes", "limits.max_output_bytes"),
    ];
    for (field, name) in fields {
        let zeroed = limits_with(field, "0");
        let payload = format!(
            r#"{{"request":{{"transcode":{{"job_dir":"/s/j","plan":"remux","limits":{zeroed}}}}}}}"#
        );

        assert_eq!(refused(payload.as_bytes()), ViolationKind::OutOfRange(name));
    }
}

#[test]
fn the_reply_text_fields_are_range_checked() {
    let caps = |vips: &str, ts: &str, sessions: &str| {
        format!(
            r#"{{"response":{{"capabilities":{{"protocol":1,"vips":{vips},"ffmpeg":"8.0","libheif":"1.20","build_timestamp":{ts},"nvenc_probe_ok":false,"nvenc_sessions":{sessions}}}}}}}"#
        )
    };

    assert_eq!(
        refused(caps(r#""""#, "1", "0").as_bytes()),
        out_of_range("vips")
    );
    assert_eq!(
        refused(caps(r#""8 18""#, "1", "0").as_bytes()),
        out_of_range("vips")
    );
    assert_eq!(
        refused(caps(r#""8.18""#, "0", "0").as_bytes()),
        out_of_range("build_timestamp")
    );
    assert_eq!(
        refused(caps(r#""8.18""#, "4102444801", "0").as_bytes()),
        out_of_range("build_timestamp")
    );
    assert_eq!(
        refused(caps(r#""8.18""#, "1", "65").as_bytes()),
        out_of_range("nvenc_sessions")
    );
    assert_eq!(
        refused(caps(r#""8.18""#, "\"2026-11-05T00:00:00Z\"", "0").as_bytes()),
        ViolationKind::Schema
    );
    accepted(&caps(r#""8.18.0""#, "1793000000", "64"));
}

#[test]
fn the_error_and_self_check_replies_are_range_checked() {
    let failed = |code: &str| {
        format!(r#"{{"response":{{"error":{{"encoder_failed":{{"exit_code":{code}}}}}}}}}"#)
    };
    let items = |n: usize| {
        let list = vec![r#"{"name":"network","ok":true}"#; n].join(",");
        format!(r#"{{"response":{{"self_check":[{list}]}}}}"#)
    };

    assert_eq!(refused(failed("0").as_bytes()), out_of_range("exit_code"));
    assert_eq!(refused(failed("256").as_bytes()), out_of_range("exit_code"));
    assert_eq!(refused(failed("-1").as_bytes()), out_of_range("exit_code"));
    assert_eq!(
        refused(failed("2147483648").as_bytes()),
        ViolationKind::Schema
    );
    assert_eq!(refused(items(65).as_bytes()), out_of_range("self_check"));
    assert_eq!(
        refused(br#"{"response":{"self_check":[{"name":"","ok":true}]}}"#),
        out_of_range("self_check.name")
    );
    accepted(&failed("255"));
    accepted(&items(64));
}

#[test]
fn limits_above_the_runner_ceilings_are_accepted_unchanged() {
    let payload = r#"{"request":{"transcode":{"job_dir":"/s/j","plan":"remux","limits":{"cpu":9999,"memory_bytes":1099511627776,"wall":86400000,"stall_timeout":18446744073709551615,"max_output_bytes":18446744073709551615}}}}"#;

    let message = accepted(payload);

    let Message::Request(Request::Transcode { limits, .. }) = message else {
        panic!("not a transcode");
    };
    assert_eq!(limits.cpu, 9999);
    assert_eq!(limits.memory_bytes, 1 << 40);
    assert_eq!(limits.wall, 86_400_000);
    assert_eq!(limits.stall_timeout, u64::MAX);
    assert_eq!(limits.max_output_bytes, u64::MAX);
}

#[test]
fn hello_accepts_any_protocol_number_so_the_runner_can_answer_mismatch() {
    assert_eq!(
        accepted(r#"{"request":{"hello":{"protocol":99}}}"#),
        Message::Request(Request::Hello { protocol: 99 })
    );
    assert_eq!(
        refused(br#"{"request":{"hello":{"protocol":-1}}}"#),
        ViolationKind::Schema
    );
}
