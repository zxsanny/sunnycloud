#![allow(dead_code)]
//! Shared pieces of the feed tests: a client on a fixture with the long-poll cap and timings the test picks,
//! page builders, and the host-side resync order (head cursor first, then the listing).

use std::time::Duration;

use serde_json::json;
use sync_core::{HttpSyncClient, SyncApi, SyncConfig};
use sync_core_proto::{
    ChangeItem, ChangePage, ChangeState, Cursor, ListingPage, NodeKind, NodeView,
};
use uuid::Uuid;

use crate::connect_support::{client, config, connect, with};
use crate::support::{Answer, Fixture, Route, Script};

/// Connects to a fixture whose `server-info` caps the long-poll at `cap_seconds`; `tune` adjusts the config.
pub async fn connected_with(
    script: Script,
    cap_seconds: u64,
    tune: impl FnOnce(SyncConfig) -> SyncConfig,
) -> (HttpSyncClient, Fixture) {
    let info = with("longpoll_max_wait", json!(cap_seconds));
    let script = script.always(Route::ServerInfo, Answer::json(200, &info));
    let fixture = Fixture::start(script).await;
    let config = tune(config(&fixture.url()));
    let (api, _) = client(connect(&fixture, config).await);
    (api, fixture)
}

/// A client with 10 ms .. 100 ms backoff and a 30 s cap, the defaults of the test config.
pub async fn connected(script: Script) -> (HttpSyncClient, Fixture) {
    connected_with(script, 30, |config| config).await
}

pub fn node(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

pub fn item(seq: u64, node_n: u128) -> ChangeItem {
    ChangeItem {
        node_id: node(node_n),
        parent_id: Some(node(1)),
        kind: NodeKind::Folder,
        name: format!("folder-{node_n}"),
        state: ChangeState::Live,
        revision_id: None,
        size: None,
        content_hash: None,
        seq,
    }
}

pub fn page(items: Vec<ChangeItem>, next: Cursor, has_more: bool) -> ChangePage {
    ChangePage {
        items,
        next_cursor: next,
        has_more,
    }
}

pub fn page_answer(page: &ChangePage) -> Answer {
    Answer::json(200, page)
}

pub fn head_answer(cursor: Cursor) -> Answer {
    Answer::json(200, &json!({ "cursor": cursor.to_string() }))
}

pub fn root_answer() -> Answer {
    Answer::json(
        200,
        &NodeView {
            node_id: node(1),
            parent_id: None,
            kind: NodeKind::Folder,
            name: String::new(),
            current_revision: None,
        },
    )
}

pub fn empty_listing() -> Answer {
    Answer::json(
        200,
        &ListingPage {
            items: Vec::new(),
            next: None,
        },
    )
}

/// What a host does after `ResyncRequired` (F14): the head cursor first, then the tree.
pub async fn resync(api: &HttpSyncClient) -> (Cursor, ListingPage) {
    let head = api.head_cursor().await.expect("the head cursor");
    let root = api.root_node().await.expect("the root node");
    let listing = api
        .list_children(root.node_id, None)
        .await
        .expect("the first listing page");
    (head, listing)
}

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

pub fn target_param(target: &str, name: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_owned))
}
