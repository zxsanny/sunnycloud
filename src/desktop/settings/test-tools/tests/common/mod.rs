#![allow(dead_code)]

use std::time::Duration;

use ipc_schema::{
    AccountView, CallError, DaemonStatus, HoldKind, HoldView, NodeId, PairId, PairState, PairView,
    Response,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sunnycloud_settings_testkit::client::RawClient;
use sunnycloud_settings_testkit::script::Script;
use sunnycloud_settings_testkit::stub::StubDaemon;

pub const WAIT: Duration = Duration::from_secs(5);

pub fn node(n: u8) -> NodeId {
    NodeId::parse(&format!("0190f3b0-7c2e-7a41-9d5e-2b6c8a1f4e{n:02x}")).expect("canonical uuid")
}

pub fn pair(id: i64, root: &str, state: PairState) -> PairView {
    PairView {
        pair_id: PairId(id),
        local_root: root.to_owned(),
        server_node_id: node(id as u8),
        state,
        rules_text: String::new(),
    }
}

pub fn status(account: bool, pairs: Vec<PairView>, holds: Vec<HoldView>) -> DaemonStatus {
    DaemonStatus {
        server_url: Some("https://cloud.example".to_owned()),
        account: account.then(|| AccountView {
            account_id: "acct-1".to_owned(),
        }),
        pairs,
        skipped_count: 0,
        escaped_count: 0,
        holds,
        update_available: None,
        access_denied: Vec::new(),
        watcher_degraded: Vec::new(),
    }
}

/// Signed in, pair 1 active at `/home/u/Sync`, pair 2 in a mass-delete hold.
pub fn signed_in() -> DaemonStatus {
    status(
        true,
        vec![
            pair(1, "/home/u/Sync", PairState::Active),
            pair(2, "/home/u/Photos", PairState::HoldMassDelete),
        ],
        vec![HoldView {
            pair_id: PairId(2),
            kind: HoldKind::MassDelete,
            local_deletes: 200,
            remote_deletes: 0,
        }],
    )
}

pub fn script_with(status: DaemonStatus) -> Script {
    Script {
        status: Some(status),
        ..Script::default()
    }
}

pub async fn start(script: Script) -> (StubDaemon, RawClient) {
    let stub = StubDaemon::start(script).await.expect("stub starts");
    let client = RawClient::connect(stub.endpoint())
        .await
        .expect("client connects");
    (stub, client)
}

pub async fn call(client: &mut RawClient, method: &str, params: Value) -> Response {
    client
        .call(method, params, WAIT)
        .await
        .unwrap_or_else(|e| panic!("{method} got no response: {e}"))
}

pub async fn ok<T: DeserializeOwned>(client: &mut RawClient, method: &str, params: Value) -> T {
    call(client, method, params)
        .await
        .into_result()
        .unwrap_or_else(|e| panic!("{method} failed: {e}"))
}

/// The `ControlError` a call failed with.
pub async fn control_error(
    client: &mut RawClient,
    method: &str,
    params: Value,
) -> ipc_schema::ControlError {
    match call(client, method, params).await.into_result::<Value>() {
        Err(CallError::Control(error)) => error,
        other => panic!("{method}: expected a ControlError, got {other:?}"),
    }
}
