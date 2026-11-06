//! AC-2 line schema, AC-3 (the part this crate owns: a span carries the request id to the events inside it),
//! the audit stream, text format and the sink-failure counter.

mod common;

use std::io;

use common::{START, capture, config};
use sc_error::CorrelationId;
use sc_observability::{LogFormat, log_debug, log_error, log_info, log_warn, request_span};
use serde_json::Value;
use tracing::Level;
use uuid::Uuid;

const REQUEST: &str = "0198d5a1-1111-7222-8333-444455556666";
const ACCOUNT: &str = "0198d5a1-aaaa-7bbb-8ccc-ddddeeeeffff";
const NODE: &str = "0198d5a1-0000-7000-8000-000000000001";

fn id(text: &str) -> Uuid {
    Uuid::parse_str(text).expect("uuid")
}

fn request_id() -> CorrelationId {
    CorrelationId::parse(REQUEST).expect("correlation id")
}

fn keys(line: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(line).expect("one JSON object");
    value.as_object().expect("object").keys().cloned().collect()
}

#[test]
fn ac2_a_full_event_is_one_json_line_with_exactly_the_schema_fields() {
    let error = io::Error::other("disk gone");
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::INFO), || {
        log_info!(
            "upload committed",
            request_id = request_id(),
            account_id = id(ACCOUNT),
            node_id = id(NODE),
            route = "/api/v1/nodes/{id}",
            status = 201,
            duration_ms = 42,
            err = &error
        );
    });

    let lines = sink.lines();
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(!line.contains('\n'));
    let value: Value = serde_json::from_str(line).expect("json");
    assert_eq!(value["ts"], "2026-11-06T09:11:12.123456Z");
    assert_eq!(value["level"], "info");
    assert_eq!(value["target"], "logging_schema");
    assert_eq!(value["msg"], "upload committed");
    assert_eq!(value["request_id"], REQUEST);
    assert_eq!(value["component"], "api");
    assert_eq!(value["account_id"], ACCOUNT);
    assert_eq!(value["node_id"], NODE);
    assert_eq!(value["route"], "/api/v1/nodes/{id}");
    assert_eq!(value["status"], 201);
    assert_eq!(value["duration_ms"], 42);
    assert_eq!(value["err"], "disk gone");
    assert_eq!(keys(line).len(), 12, "nothing but the schema: {line}");
    let order = [
        "\"ts\"",
        "\"level\"",
        "\"target\"",
        "\"msg\"",
        "\"request_id\"",
        "\"component\"",
        "\"account_id\"",
        "\"node_id\"",
        "\"route\"",
        "\"status\"",
        "\"duration_ms\"",
        "\"err\"",
    ];
    let positions: Vec<_> = order
        .iter()
        .map(|key| line.find(key).expect("key present"))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "fixed field order: {line}"
    );
}

#[test]
fn ac2_an_event_without_optional_fields_carries_only_the_required_ones() {
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::INFO), || {
        log_warn!("startup notice");
    });

    let line = &sink.lines()[0];
    let value: Value = serde_json::from_str(line).expect("json");
    assert_eq!(
        keys(line).len(),
        6,
        "ts, level, target, msg, request_id, component: {line}"
    );
    assert_eq!(value["request_id"], Value::Null);
    assert_eq!(value["level"], "warn");
}

#[test]
fn ac2_component_names_follow_the_config() {
    for (component, name) in [
        (sc_observability::Component::Api, "api"),
        (sc_observability::Component::Worker, "worker"),
        (sc_observability::Component::DecoderRunner, "decoder-runner"),
    ] {
        let mut cfg = config(LogFormat::Json, Level::INFO);
        cfg.component = component;
        let (sink, _, ()) = capture(cfg, || log_info!("hello"));
        let value: Value = serde_json::from_str(&sink.lines()[0]).expect("json");
        assert_eq!(value["component"], name);
    }
}

#[test]
fn ac2_levels_below_the_configured_one_are_not_written() {
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::WARN), || {
        log_debug!("quiet");
        log_info!("quiet");
        log_warn!("loud");
        log_error!("loud");
    });

    let levels: Vec<String> = sink
        .lines()
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("json")["level"].to_string())
        .collect();
    assert_eq!(levels, ["\"warn\"", "\"error\""]);
}

#[test]
fn ac3_events_inside_a_request_span_carry_its_request_id_on_every_level() {
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::ERROR), || {
        let span = request_span(request_id());
        let _entered = span.enter();
        log_error!("request failed", status = 500);
    });

    let value: Value = serde_json::from_str(&sink.lines()[0]).expect("json");
    assert_eq!(
        value["request_id"], REQUEST,
        "same id as the header and the error body"
    );
    assert_eq!(value["status"], 500);
}

#[test]
fn ac3_an_event_field_wins_over_the_span_and_outside_a_span_the_id_is_null() {
    let other = "0198d5a1-9999-7999-8999-999999999999";
    let (sink, _, ()) = capture(config(LogFormat::Json, Level::INFO), || {
        let span = request_span(request_id());
        let _entered = span.enter();
        log_info!(
            "override",
            request_id = CorrelationId::parse(other).expect("id")
        );
    });
    let value: Value = serde_json::from_str(&sink.lines()[0]).expect("json");
    assert_eq!(value["request_id"], other);
}

#[test]
fn audit_target_lines_carry_the_stream_and_the_audit_fields_only() {
    let (sink, stats, ()) = capture(config(LogFormat::Json, Level::INFO), || {
        tracing::info!(
            target: "audit",
            event = "login",
            actor_account_id = ACCOUNT,
            subject_id = NODE,
            ip = "10.0.0.7",
            detail = "{\"n\":1}",
            unknown_field = "leaked?",
            "audit"
        );
    });

    let value: Value = serde_json::from_str(&sink.lines()[0]).expect("json");
    assert_eq!(value["stream"], "audit");
    assert_eq!(value["target"], "audit");
    assert_eq!(value["event"], "login");
    assert_eq!(value["actor_account_id"], ACCOUNT);
    assert_eq!(value["ip"], "10.0.0.7");
    assert!(value.get("unknown_field").is_none());
    assert_eq!(stats.rejected_fields, 1);
}

#[test]
fn audit_fields_on_a_normal_target_are_dropped() {
    let (sink, stats, ()) = capture(config(LogFormat::Json, Level::INFO), || {
        tracing::info!(event = "login", ip = "10.0.0.7", "not audit");
    });

    let line = &sink.lines()[0];
    assert!(
        !line.contains("10.0.0.7") && !line.contains("login"),
        "{line}"
    );
    assert!(!line.contains("stream"));
    assert_eq!(stats.rejected_fields, 2);
}

#[test]
fn text_format_is_one_readable_line() {
    let (sink, _, ()) = capture(config(LogFormat::Text, Level::INFO), || {
        log_info!(
            "line one\nline two",
            request_id = request_id(),
            status = 404
        );
    });

    assert_eq!(sink.lines().len(), 1);
    let line = &sink.lines()[0];
    assert!(
        line.starts_with("2026-11-06T09:11:12.123456Z INFO "),
        "{line}"
    );
    assert!(line.contains("[api] line one line two"));
    assert!(line.contains(&format!("request_id={REQUEST}")));
    assert!(line.contains("status=404"));
}

#[test]
fn a_failing_sink_is_counted_not_hidden() {
    let clock = std::sync::Arc::new(sc_clock::ManualClock::new(START));
    let (subscriber, stats) = sc_observability::subscriber(
        config(LogFormat::Json, Level::INFO),
        clock,
        std::sync::Arc::new(common::Failing),
    );

    tracing::subscriber::with_default(subscriber, || log_info!("lost"));

    assert_eq!(stats().dropped_lines, 1);
}

#[test]
fn log_config_values_parse_from_the_config_keys() {
    assert_eq!("json".parse::<LogFormat>(), Ok(LogFormat::Json));
    assert_eq!("text".parse::<LogFormat>(), Ok(LogFormat::Text));
    assert!("yaml".parse::<LogFormat>().is_err());
    assert_eq!("debug".parse::<Level>().ok(), Some(Level::DEBUG));
}
