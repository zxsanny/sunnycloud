//! AC-3 to AC-6: scripted errors, delays and silence; exact recording; events on a schedule;
//! hostile frames.

mod common;

use std::time::{Duration, Instant};

use ipc_schema::{
    ControlError, DaemonStatus, Outcome, RequestId, Response, ServerMessage, StatusEvent,
};
use serde_json::{Value, json};
use sunnycloud_settings_testkit::client::ClientError;
use sunnycloud_settings_testkit::script::{Answer, EventSchedule, Hostile, TEN_MB};
use sunnycloud_settings_testkit::stub::MARKUP;

use common::*;

#[tokio::test]
async fn scripted_errors_arrive_exactly_and_the_queue_then_falls_back_to_the_default() {
    let script = script_with(signed_in())
        .with_answers(
            "add_pair",
            vec![Answer::Error(ControlError::RulesInvalid { line: 3 })],
        )
        .with_answers(
            "update_pair",
            vec![Answer::Error(ControlError::Internal {
                correlation_id: "c-77".to_owned(),
            })],
        );
    let (_stub, mut client) = start(script).await;
    let spec = json!({"target": {"Path": {"server_path": "A", "create_if_missing": true}},
        "local_root": "/home/u/New", "rules_text": ""});

    let rules = control_error(&mut client, "add_pair", spec.clone()).await;
    let internal = control_error(
        &mut client,
        "update_pair",
        json!({"pair_id": 1, "patch": {"rules_text": ""}}),
    )
    .await;
    let second_add: u64 = {
        let id: ipc_schema::PairId = ok(&mut client, "add_pair", spec).await;
        id.0 as u64
    };

    assert_eq!(rules, ControlError::RulesInvalid { line: 3 });
    assert_eq!(
        internal,
        ControlError::Internal {
            correlation_id: "c-77".to_owned()
        }
    );
    assert_eq!(second_add, 3);
}

#[tokio::test]
async fn a_delayed_answer_arrives_after_the_delay() {
    let script = script_with(signed_in()).with_answers(
        "get_settings",
        vec![Answer::Delay {
            ms: 4000,
            then: Box::new(Answer::Default),
        }],
    );
    let (_stub, mut client) = start(script).await;

    let started = Instant::now();
    let response = client
        .call("get_settings", json!({}), Duration::from_secs(10))
        .await
        .expect("delayed answer arrives");

    assert!(
        started.elapsed() >= Duration::from_millis(3990),
        "{:?}",
        started.elapsed()
    );
    assert!(matches!(response.outcome, Outcome::Success { .. }));
}

#[tokio::test]
async fn a_delayed_call_does_not_hold_back_the_calls_after_it() {
    let script = script_with(signed_in()).with_answers(
        "login_status",
        vec![Answer::Delay {
            ms: 600,
            then: Box::new(Answer::Default),
        }],
    );
    let (_stub, mut client) = start(script).await;
    let slow = client
        .send_request("login_status", json!({}))
        .await
        .unwrap();

    let started = Instant::now();
    let _status: DaemonStatus = ok(&mut client, "status", json!({})).await;
    let fast = started.elapsed();
    let late = client.read_message(WAIT).await.unwrap();

    assert!(fast < Duration::from_millis(300), "{fast:?}");
    assert!(matches!(late, ServerMessage::Response(r) if r.id == Some(RequestId::Number(slow))));
}

#[tokio::test]
async fn a_silent_call_gets_no_frame_and_the_next_call_is_still_answered() {
    let script = script_with(signed_in()).with_answers("status", vec![Answer::Silent]);
    let (stub, mut client) = start(script).await;

    client.send_request("status", json!({})).await.unwrap();
    let silence = client.read_line(Duration::from_millis(700)).await;
    let _next: DaemonStatus = ok(&mut client, "status", json!({})).await;

    assert!(matches!(silence, Err(ClientError::Timeout)));
    assert_eq!(stub.count("status"), 2);
}

#[tokio::test]
async fn answers_pushed_during_a_test_join_the_queue() {
    let (stub, mut client) = start(script_with(signed_in())).await;
    stub.push_answers(
        "logout",
        vec![Answer::Result(json!({"server_revoked": false}))],
    );

    let first: ipc_schema::LogoutOutcome = ok(&mut client, "logout", json!({})).await;

    assert!(!first.server_revoked);
}

#[tokio::test]
async fn a_scripted_result_of_the_wrong_type_is_reported_not_passed_on() {
    let script = script_with(signed_in()).with_answers("status", vec![Answer::Result(json!(7))]);
    let (_stub, mut client) = start(script).await;

    let response = call(&mut client, "status", json!({})).await;

    match response.outcome {
        Outcome::Failure { error } => {
            assert_eq!(error.code, ipc_schema::INTERNAL_ERROR_CODE);
            assert!(error.data.unwrap().to_string().contains("scripted result"));
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn the_rules_text_is_recorded_byte_equal_and_counted_once() {
    let (stub, mut client) = start(script_with(signed_in())).await;
    let rules = "*.log\nbuild/\n!keep.log\nпривіт ✓ 日本語\n";

    ok::<()>(
        &mut client,
        "update_pair",
        json!({"pair_id": 1, "patch": {"rules_text": rules}}),
    )
    .await;

    let records = stub.records_of("update_pair");
    assert_eq!(stub.count("update_pair"), 1);
    assert_eq!(records.len(), 1);
    let recorded = records[0].params.as_ref().unwrap()["patch"]["rules_text"]
        .as_str()
        .unwrap();
    assert_eq!(recorded.as_bytes(), rules.as_bytes());
    assert_eq!(
        records[0].params,
        Some(json!({"pair_id": 1, "patch": {"rules_text": rules}}))
    );
}

#[tokio::test]
async fn ten_scheduled_events_arrive_in_order_and_none_after_the_schedule_ends() {
    let statuses: Vec<DaemonStatus> = (1..=10)
        .map(|n| {
            let mut s = signed_in();
            s.skipped_count = n;
            s
        })
        .collect();
    let mut script = script_with(signed_in());
    script.events = Some(EventSchedule {
        interval_ms: 100,
        statuses,
    });
    let (_stub, mut client) = start(script).await;
    ok::<()>(&mut client, "subscribe", json!({})).await;

    let mut counts = Vec::new();
    for _ in 0..10 {
        let ServerMessage::Notification(event) = client.read_message(WAIT).await.unwrap() else {
            panic!("expected a notification");
        };
        let event: StatusEvent = event.as_status_event().unwrap().expect("status event");
        counts.push(event.status.skipped_count);
    }
    let after = client.read_line(Duration::from_millis(500)).await;

    assert_eq!(counts, (1..=10).collect::<Vec<u64>>());
    assert!(matches!(after, Err(ClientError::Timeout)));
}

#[tokio::test]
async fn stopping_the_events_ends_the_schedule_early() {
    let mut script = script_with(signed_in());
    script.events = Some(EventSchedule {
        interval_ms: 200,
        statuses: (0..20).map(|_| signed_in()).collect(),
    });
    let (stub, mut client) = start(script).await;
    ok::<()>(&mut client, "subscribe", json!({})).await;
    client.read_message(WAIT).await.unwrap();
    client.read_message(WAIT).await.unwrap();

    stub.stop_events();
    let after = client.read_line(Duration::from_millis(700)).await;

    assert!(matches!(after, Err(ClientError::Timeout)));
}

#[tokio::test]
async fn the_oversize_frame_is_one_valid_ten_megabyte_line() {
    let script = script_with(signed_in())
        .with_answers("status", vec![Answer::Hostile(Hostile::oversize_10mb())]);
    let (_stub, mut client) = start(script).await;
    let id = client.send_request("status", json!({})).await.unwrap();

    let line = client.read_line(Duration::from_secs(30)).await.unwrap();

    assert_eq!(line.len() + 1, TEN_MB, "the line without its newline");
    let response: Response = serde_json::from_slice(&line).expect("valid JSON-RPC");
    assert_eq!(response.id, Some(RequestId::Number(id)));
}

#[tokio::test]
async fn invalid_json_and_raw_frames_are_sent_verbatim() {
    let script = script_with(signed_in()).with_answers(
        "status",
        vec![
            Answer::Hostile(Hostile::InvalidJson),
            Answer::Hostile(Hostile::Raw("[1,2".to_owned())),
        ],
    );
    let (_stub, mut client) = start(script).await;

    client.send_request("status", json!({})).await.unwrap();
    let invalid = client.read_line(WAIT).await.unwrap();
    client.send_request("status", json!({})).await.unwrap();
    let raw = client.read_line(WAIT).await.unwrap();

    assert!(serde_json::from_slice::<Value>(&invalid).is_err());
    assert_eq!(invalid, b"{\"jsonrpc\": \"2.0\", \"id\": ");
    assert_eq!(raw, b"[1,2");
}

#[tokio::test]
async fn the_wrong_id_frame_carries_an_id_the_client_never_sent() {
    let script =
        script_with(signed_in()).with_answers("status", vec![Answer::Hostile(Hostile::WrongId)]);
    let (_stub, mut client) = start(script).await;
    let sent = client.send_request("status", json!({})).await.unwrap();

    let ServerMessage::Response(response) = client.read_message(WAIT).await.unwrap() else {
        panic!("expected a response");
    };

    assert_ne!(response.id, Some(RequestId::Number(sent)));
    assert!(response.id.is_some());
}

#[tokio::test]
async fn the_markup_error_text_is_sent_as_scripted() {
    let script = script_with(signed_in())
        .with_answers("set_server", vec![Answer::Hostile(Hostile::MarkupError)]);
    let (_stub, mut client) = start(script).await;

    let error = control_error(&mut client, "set_server", json!({"url": "https://x"})).await;

    assert_eq!(MARKUP, "<b>x</b>{@html}<script>");
    assert_eq!(
        error,
        ControlError::UpgradeRequired {
            message: MARKUP.to_owned()
        }
    );
}

#[tokio::test]
async fn paged_listings_follow_their_tokens_to_the_last_page() {
    use ipc_schema::FolderEntry;
    let page = |name: &str| {
        vec![FolderEntry {
            node_id: node(1),
            name: name.to_owned(),
        }]
    };
    let mut script = script_with(signed_in());
    script
        .folder_pages
        .insert(String::new(), vec![page("a"), page("b"), page("c")]);
    let (_stub, mut client) = start(script).await;

    let mut names = Vec::new();
    let mut token = Value::Null;
    loop {
        let result: ipc_schema::FolderPage = ok(
            &mut client,
            "list_server_folders",
            json!({"parent": null, "page": token}),
        )
        .await;
        names.extend(result.items.into_iter().map(|e| e.name));
        match result.next {
            Some(next) => token = json!(next.0),
            None => break,
        }
    }

    assert_eq!(names, ["a", "b", "c"]);
}

#[tokio::test]
async fn the_stub_adds_no_delay_of_its_own() {
    let (_stub, mut client) = start(script_with(signed_in())).await;
    let started = Instant::now();

    for _ in 0..50 {
        let _status: DaemonStatus = ok(&mut client, "status", json!({})).await;
    }

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
}
