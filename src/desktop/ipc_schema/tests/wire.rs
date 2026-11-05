mod common;

use common::*;
use ipc_schema::*;
use serde_json::{Value, json};

const CONTRACT_METHODS: [&str; 18] = [
    "status",
    "set_server",
    "list_server_folders",
    "list_pairs",
    "add_pair",
    "update_pair",
    "remove_pair",
    "pause",
    "resume",
    "list_skipped",
    "list_escaped",
    "get_settings",
    "set_settings",
    "resolve_hold",
    "login_start",
    "login_status",
    "logout",
    "subscribe",
];

fn failed(line: &[u8]) -> RpcError {
    match decode_call(line)
        .expect_err("the line must be refused")
        .outcome
    {
        Outcome::Failure { error } => error,
        Outcome::Success { .. } => panic!("a refusal carries an error"),
    }
}

fn add_pair_params(target: Value) -> Value {
    json!({"target": target, "local_root": "/home/u/Docs", "rules_text": ""})
}

#[test]
fn ac1_method_table_has_the_18_contract_names() {
    let names: Vec<&str> = METHODS.iter().map(|m| m.name).collect();

    assert_eq!(names, CONTRACT_METHODS);
}

#[test]
fn ac1_trait_declares_exactly_the_method_table() {
    let source = include_str!("../src/control.rs");
    let start = source
        .find("pub trait DaemonControl {")
        .expect("trait present");
    let body = &source[start..];
    let end = body.find("\n}\n").expect("trait ends");
    let declared: Vec<&str> = body[..end]
        .lines()
        .filter_map(|line| line.trim().strip_prefix("fn "))
        .filter_map(|rest| rest.split(['(', '<']).next())
        .collect();

    assert_eq!(declared, CONTRACT_METHODS);
}

#[test]
fn ac1_every_method_decodes_from_its_wire_name_and_reports_it() {
    for sample in all_calls() {
        let line = request_line(1, sample.name(), sample.params_value().unwrap());

        let call = decode_call(&line).expect("decodes");

        assert_eq!(call.call, sample);
        assert_eq!(call.call.name(), sample.name());
    }
}

fn all_calls() -> Vec<MethodCall> {
    let calls = vec![
        MethodCall::Status(NoParams {}),
        MethodCall::SetServer(SetServerParams {
            url: "https://x.example".to_owned(),
        }),
        MethodCall::ListServerFolders(ListServerFoldersParams {
            parent: None,
            page: None,
        }),
        MethodCall::ListPairs(NoParams {}),
        MethodCall::AddPair(PairSpec {
            target: PairTarget::NodeId {
                node_id: node(NODE_A),
            },
            local_root: "/r".to_owned(),
            rules_text: String::new(),
        }),
        MethodCall::UpdatePair(UpdatePairParams {
            pair_id: PairId(1),
            patch: PairPatch { rules_text: None },
        }),
        MethodCall::RemovePair(PairIdParams { pair_id: PairId(1) }),
        MethodCall::Pause(PairIdParams { pair_id: PairId(1) }),
        MethodCall::Resume(PairIdParams { pair_id: PairId(1) }),
        MethodCall::ListSkipped(ListEntriesParams {
            pair_id: Some(PairId(1)),
            page: None,
        }),
        MethodCall::ListEscaped(ListEntriesParams {
            pair_id: None,
            page: None,
        }),
        MethodCall::GetSettings(NoParams {}),
        MethodCall::SetSettings(SettingsPatch {
            bandwidth_limit: Some(1),
            hash_throttle: None,
            autostart: None,
            tray: None,
        }),
        MethodCall::ResolveHold(ResolveHoldParams {
            pair_id: PairId(1),
            decision: HoldDecision::ConfirmDeletes,
        }),
        MethodCall::LoginStart(LoginStartParams {
            device_name: "pc".to_owned(),
        }),
        MethodCall::LoginStatus(NoParams {}),
        MethodCall::Logout(NoParams {}),
        MethodCall::Subscribe(NoParams {}),
    ];
    let names: Vec<&str> = calls.iter().map(MethodCall::name).collect();
    assert_eq!(
        names, CONTRACT_METHODS,
        "the sample list covers the table in order"
    );
    calls
}

#[test]
fn round_trip_every_type() {
    let exercised = round_trip_all();

    assert!(exercised.len() >= 30, "{exercised:?}");
}

#[test]
fn ac3_pair_spec_with_both_targets_is_invalid_params() {
    let params = add_pair_params(json!({
        "NodeId": {"node_id": NODE_A},
        "Path": {"server_path": "/Docs", "create_if_missing": false}
    }));

    let error = failed(&request_line(5, "add_pair", params));

    assert_eq!(error.code, INVALID_PARAMS_CODE);
}

#[test]
fn ac3_pair_spec_with_neither_target_is_invalid_params() {
    let error = failed(&request_line(5, "add_pair", add_pair_params(json!({}))));

    assert_eq!(error.code, INVALID_PARAMS_CODE);
}

#[test]
fn ac3_pair_spec_without_a_target_member_is_invalid_params() {
    let error = failed(&request_line(
        5,
        "add_pair",
        json!({"local_root": "/r", "rules_text": ""}),
    ));

    assert_eq!(error.code, INVALID_PARAMS_CODE);
}

#[test]
fn ac3_pair_spec_with_one_target_decodes() {
    let by_id = decode_call(&request_line(
        5,
        "add_pair",
        add_pair_params(json!({"NodeId": {"node_id": NODE_A}})),
    ))
    .unwrap();
    let by_path = decode_call(&request_line(
        6,
        "add_pair",
        add_pair_params(json!({"Path": {"server_path": "/Docs", "create_if_missing": true}})),
    ))
    .unwrap();

    assert!(matches!(
        by_id.call,
        MethodCall::AddPair(PairSpec {
            target: PairTarget::NodeId { .. },
            ..
        })
    ));
    assert!(matches!(
        by_path.call,
        MethodCall::AddPair(PairSpec {
            target: PairTarget::Path {
                create_if_missing: true,
                ..
            },
            ..
        })
    ));
}

#[test]
fn a_node_id_that_is_not_a_uuid_is_invalid_params() {
    let error = failed(&request_line(
        5,
        "add_pair",
        add_pair_params(json!({"NodeId": {"node_id": "not-a-uuid"}})),
    ));

    assert_eq!(error.code, INVALID_PARAMS_CODE);
}

#[test]
fn ac4_every_control_error_maps_to_32000_and_back() {
    for error in control_errors() {
        let rpc = RpcError::from_control(&error).unwrap();

        assert_eq!(rpc.code, CONTROL_ERROR_CODE);
        assert_eq!(rpc.message, error.variant_name());
        let data = rpc.data.clone().expect("data present");
        assert_eq!(data["kind"], error.variant_name());
        assert_eq!(rpc.to_control(), Some(error.clone()), "{data}");
        let over_wire: RpcError =
            serde_json::from_slice(&serde_json::to_vec(&rpc).unwrap()).unwrap();
        assert_eq!(over_wire.to_control(), Some(error));
    }
}

#[test]
fn ac4_rules_invalid_data_shape() {
    let rpc = RpcError::from_control(&ControlError::RulesInvalid { line: 3 }).unwrap();

    assert_eq!(rpc.data, Some(json!({"kind": "RulesInvalid", "line": 3})));
}

#[test]
fn ac4_error_variants_carry_their_fields() {
    let upgrade = RpcError::from_control(&ControlError::UpgradeRequired {
        message: "needs 2".to_owned(),
    })
    .unwrap();
    let setting = RpcError::from_control(&ControlError::SettingInvalid {
        key: "tray".to_owned(),
    })
    .unwrap();

    assert_eq!(
        upgrade.data,
        Some(json!({"kind": "UpgradeRequired", "message": "needs 2"}))
    );
    assert_eq!(
        setting.data,
        Some(json!({"kind": "SettingInvalid", "key": "tray"}))
    );
}

#[test]
fn ac4_unknown_method_is_32601_with_the_request_id() {
    let response = decode_call(&request_line(8, "get_token", json!({}))).expect_err("refused");

    assert_eq!(response.id, Some(RequestId::Number(8)));
    assert!(
        matches!(response.outcome, Outcome::Failure { error } if error.code == METHOD_NOT_FOUND_CODE)
    );
}

#[test]
fn ac4_unparsable_line_is_32700_with_null_id() {
    let response = decode_call(b"{not json").expect_err("refused");

    assert_eq!(response.id, None);
    assert!(
        matches!(response.outcome, Outcome::Failure { ref error } if error.code == PARSE_ERROR_CODE)
    );
    assert!(
        serde_json::to_string(&response)
            .unwrap()
            .contains("\"id\":null")
    );
}

#[test]
fn ac4_invalid_utf8_is_a_parse_error() {
    let response = decode_call(&[b'{', 0xff, 0xfe, b'}']).expect_err("refused");

    assert!(
        matches!(response.outcome, Outcome::Failure { error } if error.code == PARSE_ERROR_CODE)
    );
}

#[test]
fn json_that_is_not_a_request_is_invalid_request() {
    let not_a_request = decode_call(b"[1,2,3]").expect_err("refused");
    let wrong_version =
        decode_call(br#"{"jsonrpc":"1.0","id":1,"method":"status"}"#).expect_err("refused");

    assert!(
        matches!(not_a_request.outcome, Outcome::Failure { error } if error.code == INVALID_REQUEST_CODE)
    );
    assert!(
        matches!(wrong_version.outcome, Outcome::Failure { error } if error.code == INVALID_REQUEST_CODE)
    );
    assert_eq!(wrong_version.id, Some(RequestId::Number(1)));
}

#[test]
fn params_may_be_absent_or_null_for_methods_without_params() {
    let absent = decode_call(br#"{"jsonrpc":"2.0","id":"a","method":"status"}"#).unwrap();
    let null =
        decode_call(br#"{"jsonrpc":"2.0","id":"b","method":"logout","params":null}"#).unwrap();

    assert_eq!(absent.id, RequestId::Text("a".to_owned()));
    assert_eq!(absent.call, MethodCall::Status(NoParams {}));
    assert_eq!(null.call, MethodCall::Logout(NoParams {}));
}

#[test]
fn node_id_parse_accepts_only_canonical_uuids() {
    assert!(NodeId::parse(NODE_A).is_ok());
    assert!(NodeId::parse("0B9F2C1E-3D4A-4F5B-8C6D-7E8F9A0B1C2D").is_ok());
    for bad in [
        "",
        "0b9f2c1e3d4a4f5b8c6d7e8f9a0b1c2d",
        "0b9f2c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2",
        "zb9f2c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2d",
        "{0b9f2c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2d}",
    ] {
        assert_eq!(NodeId::parse(bad), Err(InvalidNodeId), "{bad}");
    }
}
