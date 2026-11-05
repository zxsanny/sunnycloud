mod common;

use common::*;
use ipc_schema::*;
use serde_json::{Value, json};

#[test]
fn ac5_rules_text_survives_framing_byte_for_byte() {
    let rules = "# правила\n*.tmp\n!важливо.tmp\n\u{1F600}\ttab \"quoted\" \\ back\r\n";
    let call = MethodCall::AddPair(PairSpec {
        target: PairTarget::NodeId {
            node_id: node(NODE_A),
        },
        local_root: "/r".to_owned(),
        rules_text: rules.to_owned(),
    });
    let request = Request::new(RequestId::Number(1), &call).unwrap();

    let line = encode_line(&request).unwrap();

    assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1, "one line");
    assert_eq!(line.last(), Some(&b'\n'));
    let mut buffer = LineBuffer::default();
    buffer.push(&line);
    let decoded = decode_call(&buffer.next_line().unwrap()).unwrap();
    match decoded.call {
        MethodCall::AddPair(spec) => assert_eq!(spec.rules_text.as_bytes(), rules.as_bytes()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ac5_two_requests_on_one_stream_decode_as_two_messages_across_chunk_boundaries() {
    let mut stream = encode_line(
        &Request::new(
            RequestId::Number(1),
            &MethodCall::Pause(PairIdParams { pair_id: PairId(1) }),
        )
        .unwrap(),
    )
    .unwrap();
    stream.extend(
        encode_line(
            &Request::new(
                RequestId::Number(2),
                &MethodCall::Resume(PairIdParams { pair_id: PairId(1) }),
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut buffer = LineBuffer::default();
    let mut calls = Vec::new();

    for chunk in stream.chunks(7) {
        buffer.push(chunk);
        while let Some(line) = buffer.next_line() {
            calls.push(decode_call(&line).unwrap());
        }
    }

    let names: Vec<&str> = calls.iter().map(|c| c.call.name()).collect();
    assert_eq!(names, ["pause", "resume"]);
    assert_eq!(calls[1].id, RequestId::Number(2));
    assert_eq!(buffer.next_line(), None);
}

#[test]
fn ac5_an_incomplete_line_waits_for_its_newline() {
    let mut buffer = LineBuffer::default();

    buffer.push(br#"{"jsonrpc":"2.0""#);

    assert_eq!(buffer.next_line(), None);
}

#[test]
fn ac6_daemon_status_with_unknown_fields_decodes() {
    let mut value = serde_json::to_value(full_status()).unwrap();
    value["future_field"] = json!({"nested": [1, 2]});
    value["pairs"][0]["future_pair_field"] = json!(true);

    let status: DaemonStatus = serde_json::from_value(value).unwrap();

    assert_eq!(status, full_status());
}

#[test]
fn ac6_unknown_fields_are_ignored_in_events_and_params_too() {
    let event = json!({"status": serde_json::to_value(empty_status()).unwrap(), "extra": 1});
    let params = json!({"pair_id": 4, "extra": "x"});

    let decoded: StatusEvent = serde_json::from_value(event).unwrap();
    let call = decode_call(&request_line(1, "pause", params)).unwrap();

    assert_eq!(decoded.status, empty_status());
    assert_eq!(
        call.call,
        MethodCall::Pause(PairIdParams { pair_id: PairId(4) })
    );
}

#[test]
fn response_round_trips_for_unit_and_typed_results() {
    let unit = Response::success(RequestId::Number(1), &()).unwrap();
    let typed = Response::success(RequestId::Text("x".to_owned()), &LoginState::Pending).unwrap();

    for response in [unit, typed] {
        let line = encode_line(&response).unwrap();
        let back: ServerMessage = serde_json::from_slice(&line).unwrap();
        assert_eq!(back, ServerMessage::Response(response));
    }
}

#[test]
fn client_reads_typed_results_and_control_errors() {
    let ok = Response::from_outcome(RequestId::Number(1), Ok(settings()));
    let refused = Response::from_outcome::<()>(
        RequestId::Number(2),
        Err(ControlError::RulesInvalid { line: 3 }),
    );

    assert_eq!(ok.into_result::<Settings>().unwrap(), settings());
    assert!(matches!(
        refused.into_result::<()>(),
        Err(CallError::Control(ControlError::RulesInvalid { line: 3 }))
    ));
}

#[test]
fn client_keeps_an_unknown_control_error_as_a_plain_rpc_error() {
    let future_error = RpcError {
        code: CONTROL_ERROR_CODE,
        message: "QuotaFrozen".to_owned(),
        data: Some(json!({"kind": "QuotaFrozen"})),
    };

    let result =
        Response::failure(Some(RequestId::Number(1)), future_error.clone()).into_result::<()>();

    assert!(matches!(result, Err(CallError::Rpc(e)) if e == future_error));
}

#[test]
fn client_flags_a_result_of_the_wrong_shape() {
    let response = Response::success(RequestId::Number(1), &json!({"unexpected": 1})).unwrap();

    assert!(matches!(
        response.into_result::<Settings>(),
        Err(CallError::Malformed(_))
    ));
}

#[test]
fn method_result_validation_follows_the_table() {
    let status = serde_json::to_value(full_status()).unwrap();

    assert!(
        MethodCall::Status(NoParams {})
            .validate_result(status.clone())
            .is_ok()
    );
    assert!(
        MethodCall::GetSettings(NoParams {})
            .validate_result(status)
            .is_err()
    );
    assert!(
        MethodCall::Pause(PairIdParams { pair_id: PairId(1) })
            .validate_result(Value::Null)
            .is_ok()
    );
}

#[test]
fn event_shape_status_event_notification_carries_the_full_status() {
    let mut status = full_status();
    status.pairs[0].state = PairState::Paused;
    let notification = Notification::status_event(&StatusEvent {
        status: status.clone(),
    })
    .unwrap();

    let line = encode_line(&notification).unwrap();
    let message: ServerMessage = serde_json::from_slice(&line).unwrap();

    assert!(!String::from_utf8(line).unwrap().contains("\"id\""));
    match message {
        ServerMessage::Notification(n) => {
            assert_eq!(n.as_status_event().unwrap().unwrap().status, status);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn other_notifications_are_not_status_events() {
    let other = Notification {
        jsonrpc: Version,
        method: "something_else".to_owned(),
        params: None,
    };

    assert_eq!(other.as_status_event().unwrap(), None);
}
