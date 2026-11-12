//! AC-2: DTOs match the wire; AC-5 contribution: lane decision; NFR: parsers never panic.

mod common;

use common::Noise;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sync_core_proto::{
    BatchResult, ChangePage, ChangeState, CommitOutcome, Cursor, DeviceAuthorization, DevicePoll,
    DeviceRefusal, DeviceToken, EntryResult, ListingPage, NodeKind, RevisionView, ServerInfo,
    SessionTokens, Staged, StartOutcome, TokenType, UploadSession, effective_lanes,
};

const H1: &str = "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213";
const H2: &str = "fe1f7bc3fa20f8b474a44830b7ab8404aa4762c3dbb363d5412164ceb47e6712";
const NODE: &str = "0190c5f0-0000-7000-8000-000000000001";
const PARENT: &str = "0190c5f0-0000-7000-8000-000000000002";
const REVISION: &str = "0190c5f0-0000-7000-8000-000000000003";

fn add_unknown_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.values_mut().for_each(add_unknown_fields);
            map.insert("zz_unknown".into(), json!({"nested": [1, 2]}));
        }
        Value::Array(items) => items.iter_mut().for_each(add_unknown_fields),
        _ => {}
    }
}

/// Parses `wire`, writes the value back and requires the same JSON; the same value must also come out of
/// a copy of the JSON with an unknown field in every object.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(wire: Value) -> T {
    let parsed: T = serde_json::from_value(wire.clone()).expect("parse");
    assert_eq!(serde_json::to_value(&parsed).expect("write"), wire);
    let mut noisy = wire;
    add_unknown_fields(&mut noisy);
    let tolerant: T = serde_json::from_value(noisy).expect("parse with unknown fields");
    assert_eq!(tolerant, parsed);
    parsed
}

#[test]
fn server_info_round_trips_and_ignores_unknown_fields() {
    let wire = json!({
        "block_size": 2097152, "max_lanes": 3, "small_file_max": 2097152,
        "batch_max_entries": 250, "max_staged_per_account": 7, "longpoll_max_wait": 20,
        "min_client_version": "1.2.0", "api_versions": ["v1"]
    });

    let info: ServerInfo = round_trip(wire);

    assert_eq!(info.max_lanes, 3);
    assert_eq!(info.batch_max_entries, 250);
    assert_eq!(info.api_versions, vec!["v1"]);
}

#[test]
fn cursor_parses_to_epoch_and_seq_and_orders_by_both() {
    let high: Cursor = "1.150".parse().expect("cursor");
    let low: Cursor = "1.100".parse().expect("cursor");

    assert_eq!((high.epoch, high.seq), (1, 150));
    assert!(high > low);
    assert!("2.1".parse::<Cursor>().expect("cursor") > high);
    assert_eq!(high.to_string(), "1.150");
}

#[test]
fn malformed_cursor_texts_are_refused() {
    for text in [
        "",
        "abc",
        "1",
        "1.",
        ".1",
        "1.2.3",
        "-1.2",
        "+1.2",
        "1.+2",
        " 1.2",
        "1. 2",
        "1.2 OR 1=1",
        "1.99999999999999999999",
        "０.１",
        "1,2",
    ] {
        assert!(text.parse::<Cursor>().is_err(), "{text:?} must be refused");
    }
}

#[test]
fn change_page_round_trips() {
    let wire = json!({
        "items": [
            {"node_id": NODE, "parent_id": PARENT, "kind": "file", "name": "a.txt", "state": "live",
             "revision_id": REVISION, "size": 12, "content_hash": H1, "seq": 101},
            {"node_id": PARENT, "parent_id": null, "kind": "folder", "name": "d", "state": "tombstone",
             "revision_id": null, "size": null, "content_hash": null, "seq": 102}
        ],
        "next_cursor": "1.102",
        "has_more": true
    });

    let page: ChangePage = round_trip(wire);

    assert_eq!(page.next_cursor, Cursor::new(1, 102));
    assert_eq!(page.items[1].state, ChangeState::Tombstone);
    assert_eq!(page.items[0].kind, NodeKind::File);
    assert_eq!(page.items[0].content_hash.expect("hash").to_string(), H1);
}

#[test]
fn omitted_optional_item_fields_read_as_absent() {
    let wire = json!({
        "node_id": NODE, "kind": "folder", "name": "d", "state": "live", "seq": 5
    });

    let item: sync_core_proto::ChangeItem = serde_json::from_value(wire).expect("parse");

    assert_eq!(item.parent_id, None);
    assert_eq!(item.revision_id, None);
    assert_eq!(item.content_hash, None);
}

#[test]
fn listing_page_round_trips_with_hash_for_files_only() {
    let wire = json!({
        "items": [
            {"node_id": NODE, "parent_id": PARENT, "kind": "file", "name": "a", "current_revision": REVISION,
             "content_hash": H1},
            {"node_id": PARENT, "parent_id": null, "kind": "folder", "name": "d", "current_revision": null,
             "content_hash": null}
        ],
        "next": "page-2"
    });

    let page: ListingPage = round_trip(wire);

    assert!(page.items[0].content_hash.is_some());
    assert!(page.items[1].content_hash.is_none());
    assert_eq!(page.next.expect("token").0, "page-2");
}

#[test]
fn revision_round_trips_with_client_meta_object() {
    let wire = json!({
        "revision_id": REVISION, "blocklist": [H1, H2], "content_hash": H2, "size": 2097153,
        "client_meta": {"mtime_ns": 7}
    });

    let revision: RevisionView = serde_json::from_value(wire.clone()).expect("parse");

    assert_eq!(revision.blocklist.len(), 2);
    assert_eq!(serde_json::to_value(&revision).expect("write"), wire);
}

#[test]
fn bad_digest_text_is_refused() {
    for hash in [
        "",
        "zz",
        &H1[..63],
        &format!("{H1}0"),
        &H1.replace('2', "g"),
    ] {
        let wire = json!({
            "revision_id": REVISION, "blocklist": [hash], "content_hash": H2, "size": 1,
            "client_meta": null
        });

        assert!(
            serde_json::from_value::<RevisionView>(wire).is_err(),
            "{hash:?}"
        );
    }
}

#[test]
fn staged_round_trips_and_ignores_unknown_fields() {
    let wire = json!({"staged_key": NODE, "size": 1048576});

    let staged: Staged = round_trip(wire);

    assert_eq!(staged.size, 1_048_576);
}

#[test]
fn upload_session_and_start_outcome_round_trip() {
    let session = json!({
        "upload_id": NODE, "need_blocks": [5, 6, 9], "lane_limit": 4, "expires_at": "2026-11-05T10:00:00Z"
    });
    let parsed: UploadSession = round_trip(session);
    assert_eq!(parsed.need_blocks, vec![5, 6, 9]);

    let started: StartOutcome = round_trip(json!({
        "outcome": "session", "upload_id": NODE, "need_blocks": [], "lane_limit": 2,
        "expires_at": "2026-11-05T10:00:00Z"
    }));
    assert!(matches!(started, StartOutcome::Session(_)));

    let nothing: StartOutcome =
        round_trip(json!({"outcome": "nothing_to_send", "revision_id": REVISION}));
    assert_eq!(
        nothing,
        StartOutcome::NothingToSend {
            revision_id: REVISION.parse().expect("uuid")
        }
    );
}

#[test]
fn commit_outcome_round_trips() {
    let published: CommitOutcome =
        round_trip(json!({"outcome": "published", "node_id": NODE, "revision_id": REVISION}));
    assert!(matches!(published, CommitOutcome::Published { .. }));

    let copy: CommitOutcome =
        round_trip(json!({"outcome": "conflict_copy", "node_id": NODE, "copy_node_id": PARENT}));
    assert!(matches!(copy, CommitOutcome::ConflictCopy { .. }));
}

#[test]
fn batch_result_round_trips_both_entry_kinds() {
    let wire = json!({"entries": [
        {"status": "committed", "node_id": NODE, "revision_id": REVISION, "conflict_copy": null},
        {"status": "committed", "node_id": PARENT, "revision_id": REVISION, "conflict_copy": NODE},
        {"status": "entry_error", "code": "parent_gone"}
    ]});

    let result: BatchResult = round_trip(wire);

    assert_eq!(result.entries.len(), 3);
    assert_eq!(
        result.entries[2],
        EntryResult::EntryError {
            code: "parent_gone".into()
        }
    );
}

#[test]
fn device_flow_dtos_round_trip_in_the_server_shapes() {
    // Arrange
    let grant = json!({
        "device_code": "dc", "user_code": "ABCD-EFGH", "verification_uri": "https://s.test/device",
        "interval": 5, "expires_in": 600
    });
    let token = json!({ "access_token": "a", "token_type": "Bearer" });

    // Act
    let authorization: DeviceAuthorization = round_trip(grant);
    let token: DeviceToken = round_trip(token);

    // Assert
    assert_eq!(authorization.verification_uri, "https://s.test/device");
    assert_eq!((authorization.interval, authorization.expires_in), (5, 600));
    assert_eq!(token.access_token, "a");
    assert_eq!(token.token_type, TokenType::Bearer);
}

#[test]
fn a_grant_without_expires_in_or_a_token_of_another_type_is_refused() {
    // Arrange
    let grant = json!({
        "device_code": "dc", "user_code": "ABCD-EFGH", "verification_uri": "https://s.test/device",
        "interval": 5
    });
    let token = json!({ "access_token": "a", "token_type": "MAC" });

    // Act
    let grant = serde_json::from_value::<DeviceAuthorization>(grant);
    let token = serde_json::from_value::<DeviceToken>(token);

    // Assert
    assert!(grant.is_err());
    assert!(token.is_err());
}

#[test]
fn the_four_rfc_8628_codes_are_the_four_poll_outcomes_and_nothing_else_is() {
    // Arrange
    let codes = [
        "authorization_pending",
        "slow_down",
        "access_denied",
        "expired_token",
        "invalid_request",
        "pending",
    ];

    // Act
    let outcomes: Vec<Option<DevicePoll>> = codes
        .iter()
        .map(|code| DeviceRefusal::from_code(code).map(DevicePoll::from))
        .collect();

    // Assert
    assert_eq!(
        outcomes,
        [
            Some(DevicePoll::Pending),
            Some(DevicePoll::SlowDown),
            Some(DevicePoll::Denied),
            Some(DevicePoll::Expired),
            None,
            None,
        ]
    );
}

#[test]
fn token_debug_never_prints_a_token() {
    // Arrange
    let tokens = SessionTokens {
        access: "ACCESS-SECRET".into(),
        refresh: Some("REFRESH-SECRET".into()),
        expires_in: 900,
    };
    let device = DeviceToken {
        access_token: "DEVICE-SECRET".into(),
        token_type: TokenType::Bearer,
    };

    // Act
    let printed = format!("{tokens:?} {:?}", DevicePoll::Token(device));

    // Assert
    assert!(!printed.contains("SECRET"), "{printed}");
}

#[test]
fn effective_lanes_is_the_lowest_of_cap_server_and_session() {
    assert_eq!(effective_lanes(8, 8), 4);
    assert_eq!(effective_lanes(2, 4), 2);
    assert_eq!(effective_lanes(4, 1), 1);
    assert_eq!(effective_lanes(3, 3), 3);
}

fn random_text(noise: &mut Noise) -> String {
    const PIECES: &[&str] = &[
        "{",
        "}",
        "[",
        "]",
        ":",
        ",",
        "\"",
        "null",
        "true",
        "1",
        "-1",
        "1.5",
        "1.100",
        "18446744073709551616",
        "epoch",
        "seq",
        "items",
        "kind",
        "file",
        "node_id",
        NODE,
        H1,
        "\\u0000",
        "é",
        " ",
        ".",
        "+",
    ];
    (0..noise.below(24))
        .map(|_| PIECES[noise.below(PIECES.len())])
        .collect()
}

#[test]
fn parsers_never_panic_on_random_text() {
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
    let mut accepted = 0;
    let mut refused = 0;
    for _ in 0..5_000 {
        let text = random_text(&mut noise);
        let results = [
            text.parse::<Cursor>().is_ok(),
            serde_json::from_str::<ChangePage>(&text).is_ok(),
            serde_json::from_str::<ServerInfo>(&text).is_ok(),
            serde_json::from_str::<RevisionView>(&text).is_ok(),
            serde_json::from_str::<BatchResult>(&text).is_ok(),
            serde_json::from_str::<DeviceToken>(&text).is_ok(),
            serde_json::from_str::<DeviceAuthorization>(&text).is_ok(),
            sync_core_proto::ServerUrl::parse(&text, true).is_ok(),
        ];
        accepted += results.iter().filter(|ok| **ok).count();
        refused += results.iter().filter(|ok| !**ok).count();
    }
    assert!(
        refused > accepted,
        "noise should be refused far more often than accepted"
    );
}
