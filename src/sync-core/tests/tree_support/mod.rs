#![allow(dead_code)]
//! Shared pieces of the tree-operation tests: a client connected to a scripted fixture, DTO builders and
//! the request log of one route.

use serde_json::Value;
use sync_core::HttpSyncClient;
use sync_core_proto::{FileHash, ListingItem, NodeKind, RevisionRef};
use uuid::Uuid;

use crate::connect_support::{client, config, connect, valid_info};
use crate::support::{Answer, Fixture, LoggedRequest, Route, Script};

/// Connects to a fixture that serves `server-info` and `script`. The fixture is returned to keep its tasks alive.
pub async fn connected(script: Script) -> (HttpSyncClient, Fixture) {
    let script = script.always(Route::ServerInfo, Answer::json(200, &valid_info()));
    let fixture = Fixture::start(script).await;
    let (api, _) = client(connect(&fixture, config(&fixture.url())).await);
    (api, fixture)
}

/// The requests the fixture saw on `route`, in arrival order.
pub fn seen(fixture: &Fixture, route: Route) -> Vec<LoggedRequest> {
    fixture
        .requests()
        .into_iter()
        .filter(|request| request.route == Some(route))
        .collect()
}

pub fn body_json(request: &LoggedRequest) -> Value {
    serde_json::from_slice(&request.body).expect("the client sent a JSON body")
}

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

pub fn file_hash(n: u64) -> FileHash {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&n.to_le_bytes());
    bytes[31] = 0xA5;
    FileHash::from(bytes)
}

pub fn file_item(n: u64, name: &str) -> ListingItem {
    ListingItem {
        node_id: id(u128::from(n)),
        parent_id: Some(id(1)),
        kind: NodeKind::File,
        name: name.to_owned(),
        current_revision: Some(id(0x1000 + u128::from(n))),
        content_hash: Some(file_hash(n)),
    }
}

pub fn folder_item(n: u64, name: &str) -> ListingItem {
    ListingItem {
        node_id: id(u128::from(n)),
        parent_id: Some(id(1)),
        kind: NodeKind::Folder,
        name: name.to_owned(),
        current_revision: None,
        content_hash: None,
    }
}

pub fn revision_ref(n: u64) -> RevisionRef {
    RevisionRef {
        revision_id: id(0x1000 + u128::from(n)),
        node_id: id(u128::from(n)),
        content_hash: file_hash(n),
        size: n * 10,
        client_meta: Some(serde_json::json!({ "device": "phone-1", "asset": n })),
    }
}
