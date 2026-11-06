//! AC-1 (every method answered) and AC-2 (defaults match daemon_control.md).

mod common;

use ipc_schema::{
    ControlError, DaemonStatus, LoginState, LogoutOutcome, METHODS, MethodCall, PairState,
    Settings, StatusEvent,
};
use serde_json::{Value, json};

use common::*;

fn sample_params(method: &str) -> Value {
    match method {
        "status" | "list_pairs" | "get_settings" | "login_status" | "logout" | "subscribe" => {
            json!({})
        }
        "set_server" => json!({"url": "https://other.example"}),
        "list_server_folders" => json!({"parent": null, "page": null}),
        "add_pair" => {
            json!({"target": {"Path": {"server_path": "Photos", "create_if_missing": true}},
            "local_root": "/home/u/New", "rules_text": ""})
        }
        "update_pair" => json!({"pair_id": 1, "patch": {"rules_text": "*.tmp\n"}}),
        "remove_pair" | "pause" | "resume" => json!({"pair_id": 1}),
        "list_skipped" | "list_escaped" => json!({"pair_id": null, "page": null}),
        "set_settings" => json!({"bandwidth_limit": 5000}),
        "resolve_hold" => json!({"pair_id": 2, "decision": "RestoreFromServer"}),
        "login_start" => json!({"device_name": "laptop"}),
        other => panic!("the stub coverage test has no sample params for the new method `{other}`"),
    }
}

#[tokio::test]
async fn every_method_of_the_schema_is_answered_with_a_decodable_result() {
    let mut answered = Vec::new();

    for info in METHODS {
        let params = sample_params(info.name);
        // Each method gets a fresh stub, so an earlier call (remove_pair) cannot change what
        // a later one sees. set_server is refused while signed in, hence the signed-out stub.
        let initial = if info.name == "set_server" {
            status(false, vec![], vec![])
        } else {
            signed_in()
        };
        let (_stub, mut client) = start(script_with(initial)).await;
        let client = &mut client;
        let response = call(client, info.name, params.clone()).await;
        let outcome: Result<Value, _> = response.into_result();
        let result = outcome.unwrap_or_else(|e| panic!("{} was refused: {e}", info.name));
        MethodCall::decode(info.name, Some(params))
            .unwrap_or_else(|e| panic!("sample params of {} do not decode: {e:?}", info.name))
            .validate_result(result)
            .unwrap_or_else(|e| panic!("result of {} is not {}: {e}", info.name, info.result));
        answered.push(info.name);
    }

    assert_eq!(answered.len(), METHODS.len());
}

#[tokio::test]
async fn http_address_is_refused_with_https_required() {
    let (_stub, mut client) = start(script_with(status(false, vec![], vec![]))).await;

    let error = control_error(&mut client, "set_server", json!({"url": "http://x"})).await;

    assert_eq!(error, ControlError::HttpsRequired);
}

#[tokio::test]
async fn server_change_while_signed_in_is_refused() {
    let (_stub, mut client) = start(script_with(signed_in())).await;

    let error = control_error(
        &mut client,
        "set_server",
        json!({"url": "https://y.example"}),
    )
    .await;

    assert_eq!(error, ControlError::SignedIn);
}

#[tokio::test]
async fn negative_bandwidth_is_setting_invalid_and_settings_stay_unchanged() {
    let (stub, mut client) = start(script_with(signed_in())).await;
    let before: Settings = ok(&mut client, "get_settings", json!({})).await;

    let error = control_error(&mut client, "set_settings", json!({"bandwidth_limit": -1})).await;
    let after: Settings = ok(&mut client, "get_settings", json!({})).await;

    assert_eq!(
        error,
        ControlError::SettingInvalid {
            key: "bandwidth_limit".to_owned()
        }
    );
    assert_eq!(before, after);
    assert_eq!(stub.count("set_settings"), 1);
}

#[tokio::test]
async fn set_settings_echoes_the_applied_values() {
    let (_stub, mut client) = start(script_with(signed_in())).await;

    let applied: Settings = ok(
        &mut client,
        "set_settings",
        json!({"bandwidth_limit": 5000, "autostart": true}),
    )
    .await;

    assert_eq!(applied.bandwidth_limit, 5000);
    assert!(applied.autostart);
    let stored: Settings = ok(&mut client, "get_settings", json!({})).await;
    assert_eq!(stored, applied);
}

#[tokio::test]
async fn resolve_hold_on_an_active_pair_is_no_such_hold() {
    let (_stub, mut client) = start(script_with(signed_in())).await;

    let error = control_error(
        &mut client,
        "resolve_hold",
        json!({"pair_id": 1, "decision": "ConfirmDeletes"}),
    )
    .await;

    assert_eq!(error, ControlError::NoSuchHold);
}

#[tokio::test]
async fn resolving_a_hold_returns_the_pair_to_active_and_clears_the_hold() {
    let (_stub, mut client) = start(script_with(signed_in())).await;

    ok::<()>(
        &mut client,
        "resolve_hold",
        json!({"pair_id": 2, "decision": "ConfirmDeletes"}),
    )
    .await;

    let status: DaemonStatus = ok(&mut client, "status", json!({})).await;
    assert!(status.holds.is_empty());
    assert_eq!(status.pairs[1].state, PairState::Active);
}

#[tokio::test]
async fn nested_roots_and_a_second_pairing_of_one_folder_are_refused() {
    let (_stub, mut client) = start(script_with(signed_in())).await;
    let nested = json!({"target": {"Path": {"server_path": "A", "create_if_missing": true}},
        "local_root": "/home/u/Sync/inner", "rules_text": ""});
    let taken = json!({"target": {"NodeId": {"node_id": node(1).as_str()}},
        "local_root": "/home/u/Other", "rules_text": ""});

    let nested_error = control_error(&mut client, "add_pair", nested).await;
    let taken_error = control_error(&mut client, "add_pair", taken).await;

    assert_eq!(nested_error, ControlError::RootNested);
    assert_eq!(taken_error, ControlError::AlreadyPaired);
}

#[tokio::test]
async fn pairs_need_a_sign_in() {
    let (_stub, mut client) = start(script_with(status(false, vec![], vec![]))).await;

    let error = control_error(&mut client, "list_pairs", json!({})).await;

    assert_eq!(error, ControlError::NotSignedIn);
}

#[tokio::test]
async fn login_needs_a_server_address_and_then_goes_pending() {
    let mut without_server = status(false, vec![], vec![]);
    without_server.server_url = None;
    let (_stub, mut client) = start(script_with(without_server)).await;

    let error = control_error(&mut client, "login_start", json!({"device_name": "d"})).await;

    assert_eq!(error, ControlError::ServerNotSet);

    let (_stub, mut client) = start(script_with(status(false, vec![], vec![]))).await;
    let _prompt: ipc_schema::LoginPrompt =
        ok(&mut client, "login_start", json!({"device_name": "d"})).await;
    let state: LoginState = ok(&mut client, "login_status", json!({})).await;
    assert_eq!(state, LoginState::Pending);
}

#[tokio::test]
async fn logout_drops_the_account_and_reports_the_server_revoked() {
    let (_stub, mut client) = start(script_with(signed_in())).await;

    let outcome: LogoutOutcome = ok(&mut client, "logout", json!({})).await;

    assert!(outcome.server_revoked);
    let status: DaemonStatus = ok(&mut client, "status", json!({})).await;
    assert!(status.account.is_none());
}

#[tokio::test]
async fn pause_and_resume_each_emit_one_full_status_event_before_the_response() {
    let (_stub, mut client) = start(script_with(signed_in())).await;
    ok::<()>(&mut client, "subscribe", json!({})).await;

    ok::<()>(&mut client, "pause", json!({"pair_id": 1})).await;
    let after_pause = client.events.len();
    ok::<()>(&mut client, "resume", json!({"pair_id": 1})).await;

    assert_eq!(after_pause, 1);
    let states: Vec<PairState> = client
        .events
        .iter()
        .map(|n| {
            let event: StatusEvent = n.as_status_event().unwrap().expect("a status event");
            event.status.pairs[0].state
        })
        .collect();
    assert_eq!(states, vec![PairState::Paused, PairState::Active]);
}

#[tokio::test]
async fn removing_an_unknown_pair_is_unknown_pair() {
    let (_stub, mut client) = start(script_with(signed_in())).await;

    let error = control_error(&mut client, "remove_pair", json!({"pair_id": 99})).await;

    assert_eq!(error, ControlError::UnknownPair);
}
