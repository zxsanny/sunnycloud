#![allow(dead_code)]
//! Shared pieces of the `connect` tests: a config for the fixture, a valid `ServerInfo` and its variants.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use sync_core::{ConnectError, HttpSyncClient, SyncConfig};
use sync_core_proto::ServerInfo;

use crate::request_support::SentinelCredentials;
use crate::support::{Answer, Fixture, Route, Script};

pub const CLIENT_VERSION: &str = "1.2.3";

pub fn config(url: &str) -> SyncConfig {
    SyncConfig {
        server_url: url.to_owned(),
        client_family: "desktop".to_owned(),
        client_version: CLIENT_VERSION.to_owned(),
        retry_base: Duration::from_millis(10),
        retry_max: Duration::from_millis(100),
        max_attempts_per_block: 3,
        connect_timeout: Duration::from_secs(2),
        request_idle_timeout: Duration::from_secs(2),
        longpoll_wait: Duration::from_secs(40),
        longpoll_client_margin: Duration::from_secs(1),
        h2_stream_window: 8 * 1024 * 1024,
        h2_conn_window: 32 * 1024 * 1024,
        keepalive_interval: Duration::from_secs(20),
        hash_cpu_share: 25,
        hash_checkpoint_blocks: 32,
        batch_max_bytes_in_flight: 64 * 1024 * 1024,
    }
}

/// A `server-info` body every check accepts.
pub fn valid_info() -> Value {
    json!({
        "block_size": 2_097_152,
        "max_lanes": 4,
        "small_file_max": 2_097_152,
        "batch_max_entries": 500,
        "max_staged_per_account": 1000,
        "longpoll_max_wait": 30,
        "min_client_version": "1.0.0",
        "api_versions": ["v1"],
    })
}

pub fn with(field: &str, value: Value) -> Value {
    let mut info = valid_info();
    info[field] = value;
    info
}

pub fn info_script(info: &Value) -> Script {
    Script::new().always(Route::ServerInfo, Answer::json(200, info))
}

type Connected = Result<(HttpSyncClient, ServerInfo), ConnectError>;

/// `connect` to the fixture, trusting its CA.
pub async fn connect(fixture: &Fixture, config: SyncConfig) -> Connected {
    HttpSyncClient::connect_with_roots(
        config,
        SentinelCredentials::new(),
        vec![fixture.ca().clone()],
    )
    .await
}

/// `connect` to a fixture serving `info`.
pub async fn connect_to_info(info: &Value) -> Connected {
    let fixture = Fixture::start(info_script(info)).await;
    connect(&fixture, config(&fixture.url())).await
}

pub fn failure(result: Connected) -> ConnectError {
    result.err().expect("connect should fail")
}

pub fn client(result: Connected) -> (HttpSyncClient, ServerInfo) {
    result.unwrap_or_else(|error| panic!("connect should succeed: {error}"))
}

pub fn shared_credentials() -> Arc<SentinelCredentials> {
    SentinelCredentials::new()
}
