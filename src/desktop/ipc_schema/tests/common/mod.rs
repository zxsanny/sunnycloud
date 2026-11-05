#![allow(dead_code)] // each integration test binary uses a different part of this module

pub mod source_walk;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use ipc_schema::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub const NODE_A: &str = "0b9f2c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2d";
pub const NODE_B: &str = "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d";

/// Polls a future that must be ready at once; the crate has no runtime of its own.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the fake daemon must answer without waiting"),
    }
}

pub fn request_line(id: i64, method: &str, params: Value) -> Vec<u8> {
    let mut line = serde_json::to_vec(
        &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )
    .unwrap();
    line.push(b'\n');
    line
}

pub fn node(text: &str) -> NodeId {
    NodeId::parse(text).expect("sample node id is canonical")
}

pub fn pair_view(id: i64, state: PairState) -> PairView {
    PairView {
        pair_id: PairId(id),
        local_root: "/home/u/Documents".to_owned(),
        server_node_id: node(NODE_A),
        state,
        rules_text: "*.tmp\n".to_owned(),
    }
}

pub fn full_status() -> DaemonStatus {
    DaemonStatus {
        server_url: Some("https://cloud.example.org".to_owned()),
        account: Some(AccountView {
            account_id: "acc-1".to_owned(),
        }),
        pairs: vec![pair_view(1, PairState::Paused)],
        skipped_count: 4,
        escaped_count: 2,
        holds: vec![HoldView {
            pair_id: PairId(1),
            kind: HoldKind::MassDelete,
            local_deletes: 250,
            remote_deletes: 0,
        }],
        update_available: Some(UpdateView {
            version: "1.2.0".to_owned(),
        }),
        access_denied: vec![PairId(3)],
        watcher_degraded: vec![PairId(1)],
    }
}

pub fn empty_status() -> DaemonStatus {
    DaemonStatus {
        server_url: None,
        account: None,
        pairs: Vec::new(),
        skipped_count: 0,
        escaped_count: 0,
        holds: Vec::new(),
        update_available: None,
        access_denied: Vec::new(),
        watcher_degraded: Vec::new(),
    }
}

pub fn settings() -> Settings {
    Settings {
        bandwidth_limit: 512,
        hash_throttle: 0.25,
        autostart: true,
        tray: false,
    }
}

pub fn control_errors() -> Vec<ControlError> {
    vec![
        ControlError::HttpsRequired,
        ControlError::ServerUnreachable,
        ControlError::UpgradeRequired {
            message: "server needs 2.0".to_owned(),
        },
        ControlError::SignedIn,
        ControlError::NotSignedIn,
        ControlError::ServerNotSet,
        ControlError::UnknownFolder,
        ControlError::RootInvalid,
        ControlError::RootNested,
        ControlError::RulesInvalid { line: 3 },
        ControlError::CloudManagedRoot,
        ControlError::AlreadyPaired,
        ControlError::UnknownPair,
        ControlError::SettingInvalid {
            key: "bandwidth_limit".to_owned(),
        },
        ControlError::NoSuchHold,
        ControlError::Internal {
            correlation_id: "c-42".to_owned(),
        },
    ]
}

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let json = serde_json::to_string(value).expect("serializes");
    let back: T = serde_json::from_str(&json).expect("decodes");
    assert_eq!(&back, value, "round trip through {json}");
}

fn record<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    seen: &mut BTreeMap<&'static str, usize>,
    name: &'static str,
    values: Vec<T>,
) {
    for value in &values {
        round_trip(value);
    }
    seen.insert(name, values.len());
}

/// Round-trips every type of the schema through JSON. The result maps a type name to the
/// number of values exercised (for an enum, its variants), so a test can compare it with the
/// definitions.
pub fn round_trip_all() -> BTreeMap<&'static str, usize> {
    let mut seen = BTreeMap::new();
    let s = &mut seen;
    record(s, "NodeId", vec![node(NODE_A)]);
    record(s, "PairId", vec![PairId(-7), PairId(9)]);
    record(s, "PageToken", vec![PageToken("opaque==".to_owned())]);
    record(
        s,
        "FolderEntry",
        vec![FolderEntry {
            node_id: node(NODE_B),
            name: "Фото".to_owned(),
        }],
    );
    record(
        s,
        "FolderPage",
        vec![FolderPage {
            items: vec![FolderEntry {
                node_id: node(NODE_B),
                name: "docs".to_owned(),
            }],
            next: Some(PageToken("n".to_owned())),
        }],
    );
    record(
        s,
        "PairTarget",
        vec![
            PairTarget::NodeId {
                node_id: node(NODE_A),
            },
            PairTarget::Path {
                server_path: "/Docs/Work".to_owned(),
                create_if_missing: true,
            },
        ],
    );
    record(
        s,
        "PairSpec",
        vec![PairSpec {
            target: PairTarget::NodeId {
                node_id: node(NODE_A),
            },
            local_root: "C:\\Users\\u\\Docs".to_owned(),
            rules_text: "*.log\n!keep.log\n".to_owned(),
        }],
    );
    record(
        s,
        "PairPatch",
        vec![
            PairPatch {
                rules_text: Some("a\n".to_owned()),
            },
            PairPatch { rules_text: None },
        ],
    );
    record(
        s,
        "PairState",
        vec![
            PairState::Active,
            PairState::Paused,
            PairState::HoldMassDelete,
            PairState::RootMissing,
        ],
    );
    record(s, "PairView", vec![pair_view(1, PairState::Active)]);
    record(
        s,
        "HoldKind",
        vec![HoldKind::MassDelete, HoldKind::RootMissing],
    );
    record(
        s,
        "HoldDecision",
        vec![
            HoldDecision::ConfirmDeletes,
            HoldDecision::RestoreFromServer,
        ],
    );
    record(s, "HoldView", vec![full_status().holds[0].clone()]);
    record(
        s,
        "SkipReason",
        vec![
            SkipReason::Placeholder,
            SkipReason::Symlink,
            SkipReason::Junction,
            SkipReason::SpecialFile,
        ],
    );
    let skipped = SkippedEntry {
        pair_id: PairId(1),
        rel_path: "a/b".to_owned(),
        reason: SkipReason::Symlink,
    };
    record(s, "SkippedEntry", vec![skipped.clone()]);
    record(
        s,
        "SkippedPage",
        vec![SkippedPage {
            items: vec![skipped],
            next: None,
        }],
    );
    let escaped = EscapedEntry {
        pair_id: PairId(1),
        original_name: "a:b".to_owned(),
        saved_name: "a%3Ab".to_owned(),
        rel_path: "d/a%3Ab".to_owned(),
    };
    record(s, "EscapedEntry", vec![escaped.clone()]);
    record(
        s,
        "EscapedPage",
        vec![EscapedPage {
            items: vec![escaped],
            next: Some(PageToken("e".to_owned())),
        }],
    );
    record(s, "Settings", vec![settings()]);
    record(
        s,
        "SettingsPatch",
        vec![
            SettingsPatch {
                bandwidth_limit: Some(0),
                hash_throttle: Some(0.5),
                autostart: Some(false),
                tray: Some(true),
            },
            SettingsPatch {
                bandwidth_limit: None,
                hash_throttle: None,
                autostart: None,
                tray: None,
            },
        ],
    );
    record(
        s,
        "AccountView",
        vec![AccountView {
            account_id: "acc".to_owned(),
        }],
    );
    record(
        s,
        "UpdateView",
        vec![UpdateView {
            version: "1.0.1".to_owned(),
        }],
    );
    record(s, "DaemonStatus", vec![full_status(), empty_status()]);
    record(
        s,
        "StatusEvent",
        vec![StatusEvent {
            status: full_status(),
        }],
    );
    record(
        s,
        "LoginPrompt",
        vec![LoginPrompt {
            verification_url: "https://cloud.example.org/device".to_owned(),
            user_code: "WDJB-MJHT".to_owned(),
            expires_at: "2026-11-05T10:00:00Z".to_owned(),
        }],
    );
    record(
        s,
        "LoginState",
        vec![
            LoginState::Pending,
            LoginState::Approved,
            LoginState::Denied,
            LoginState::Expired,
            LoginState::SignedOut,
        ],
    );
    record(
        s,
        "LogoutOutcome",
        vec![LogoutOutcome {
            server_revoked: false,
        }],
    );
    record(s, "ControlError", control_errors());
    record(s, "NoParams", vec![NoParams {}]);
    record(
        s,
        "SetServerParams",
        vec![SetServerParams {
            url: "https://x.example".to_owned(),
        }],
    );
    record(
        s,
        "ListServerFoldersParams",
        vec![ListServerFoldersParams {
            parent: Some(node(NODE_A)),
            page: None,
        }],
    );
    record(
        s,
        "UpdatePairParams",
        vec![UpdatePairParams {
            pair_id: PairId(2),
            patch: PairPatch {
                rules_text: Some("x".to_owned()),
            },
        }],
    );
    record(s, "PairIdParams", vec![PairIdParams { pair_id: PairId(2) }]);
    record(
        s,
        "ListEntriesParams",
        vec![ListEntriesParams {
            pair_id: None,
            page: Some(PageToken("p".to_owned())),
        }],
    );
    record(
        s,
        "ResolveHoldParams",
        vec![ResolveHoldParams {
            pair_id: PairId(2),
            decision: HoldDecision::RestoreFromServer,
        }],
    );
    record(
        s,
        "LoginStartParams",
        vec![LoginStartParams {
            device_name: "laptop".to_owned(),
        }],
    );
    seen
}
