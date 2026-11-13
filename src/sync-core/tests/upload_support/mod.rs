#![allow(dead_code)]
//! Shared pieces of the large-upload tests: a client connected to the scripted fixture, hashed sources, the
//! answers of the upload routes, and readings of the request log.

use std::sync::Arc;

use serde_json::{Value, json};
use sync_core::{
    FileSource, HashedFile, HttpSyncClient, MemoryCheckpointStore, SyncConfig, UploadRequest,
};
use sync_core_proto::{BLOCK_SIZE, Hash32, hash_block, rollup};
use uuid::Uuid;

use crate::connect_support::{client, config, connect, valid_info};
use crate::hash_support::{BLOCK, Content, GeneratedSource, KEY};
use crate::support::{Answer, Fixture, LoggedRequest, Route, Script};
use crate::tree_support::{body_json, id, seen};

pub const UPLOAD: Uuid = Uuid::from_u128(0x0A);
pub const PARENT: Uuid = Uuid::from_u128(0x01);

/// A client with the test config changed by `tune`, connected to a fixture that serves `script`.
pub async fn connected_with(
    script: Script,
    tune: impl FnOnce(SyncConfig) -> SyncConfig,
) -> (HttpSyncClient, Fixture) {
    let script = script.always(Route::ServerInfo, Answer::json(200, &valid_info()));
    let fixture = Fixture::start(script).await;
    let config = tune(config(&fixture.url()));
    let (api, _) = client(connect(&fixture, config).await);
    (api, fixture)
}

/// The test config: 10 ms .. 100 ms backoff, 3 sends per block.
pub async fn connected(script: Script) -> (HttpSyncClient, Fixture) {
    connected_with(script, |config| config).await
}

/// A UUIDv7-shaped key, as a desktop host creates one.
pub fn key(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n)
}

/// The block hashes and file hash of a generated source of `len` bytes, computed without reading it.
pub fn hashed(len: u64, content: Content) -> HashedFile {
    let blocks: Vec<Hash32> = (0..len.div_ceil(BLOCK))
        .map(|block| {
            let start = block * BLOCK;
            let end = (start + BLOCK).min(len);
            let data: Vec<u8> = match content {
                Content::Pattern => (start..end).map(|i| (i % 251) as u8).collect(),
                Content::BlockFill => vec![(block % 251) as u8; (end - start) as usize],
            };
            hash_block(&data).expect("a block is at most 2 MiB")
        })
        .collect();
    HashedFile {
        file_hash: rollup(&blocks),
        block_hashes: blocks,
    }
}

/// A generated source of `blocks` full blocks and its hashes.
pub fn file(blocks: u64, content: Content) -> (Arc<GeneratedSource>, HashedFile) {
    let len = blocks * BLOCK;
    let source = Arc::new(GeneratedSource::with_content(len, content));
    (source, hashed(len, content))
}

pub fn request(
    source: Arc<dyn FileSource>,
    hashed: HashedFile,
    client_key: Uuid,
    base_revision: Option<Uuid>,
) -> UploadRequest {
    UploadRequest {
        local_key: KEY.to_owned(),
        parent: PARENT,
        name: "report.bin".to_owned(),
        source,
        hashed,
        base_revision,
        client_key,
        client_meta: None,
        import_hint: false,
    }
}

pub fn store() -> Arc<MemoryCheckpointStore> {
    Arc::new(MemoryCheckpointStore::new())
}

/// `start_upload` opening (or finding) session [`UPLOAD`] that lacks `need_blocks`.
pub fn session(need_blocks: &[u32], lane_limit: u32) -> Answer {
    Answer::json(
        200,
        &json!({
            "outcome": "session",
            "upload_id": UPLOAD,
            "need_blocks": need_blocks,
            "block_size": BLOCK_SIZE,
            "lane_limit": lane_limit,
            "expires_at": "2026-11-14T12:00:00Z",
        }),
    )
}

pub fn every_block(blocks: u32) -> Vec<u32> {
    (0..blocks).collect()
}

pub fn nothing_to_send(revision: Uuid) -> Answer {
    Answer::json(
        200,
        &json!({ "outcome": "nothing_to_send", "revision_id": revision }),
    )
}

pub fn published(node: Uuid, revision: Uuid) -> Answer {
    Answer::json(
        200,
        &json!({ "outcome": "published", "node_id": node, "revision_id": revision }),
    )
}

pub fn conflict_copy(node: Uuid, revision: Uuid, copy: Uuid) -> Answer {
    Answer::json(
        200,
        &json!({
            "outcome": "conflict_copy",
            "node_id": node,
            "revision_id": revision,
            "copy_node_id": copy,
        }),
    )
}

/// The bodies of every `start_upload`, in order.
pub fn start_bodies(fixture: &Fixture) -> Vec<Value> {
    seen(fixture, Route::StartUpload)
        .iter()
        .map(body_json)
        .collect()
}

pub fn client_keys(fixture: &Fixture) -> Vec<Value> {
    start_bodies(fixture)
        .into_iter()
        .map(|body| body["client_key"].clone())
        .collect()
}

/// Block indexes of the block `PUT`s among `requests`, in arrival order.
pub fn block_indexes(requests: &[LoggedRequest]) -> Vec<u32> {
    requests
        .iter()
        .filter(|request| request.route == Some(Route::PutBlock))
        .filter_map(LoggedRequest::block_index)
        .collect()
}

pub fn sends_of(fixture: &Fixture, index: u32) -> usize {
    block_indexes(&fixture.requests())
        .into_iter()
        .filter(|sent| *sent == index)
        .count()
}

pub fn hex(hash: &impl std::fmt::Display) -> Value {
    Value::from(hash.to_string())
}

pub fn node(n: u128) -> Uuid {
    id(n)
}
