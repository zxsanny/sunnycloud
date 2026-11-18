//! Flow F5 against the real `sunnycloud serve` binary on PostgreSQL 18 and a blob volume (`serve_support`): a
//! signed-in device of a new account, staged small files over HTTP, `commits:batch`, the folder listing and the
//! feed head over HTTP, and probes of what the database stores (quota reservations, staged rows).
//!
//! Parallel staging runs as concurrent streams on one h2c connection (`h2`); single requests use the serve
//! harness's HTTP/1.1 client.

#![allow(dead_code)]

pub mod download;
pub mod feed;
pub mod h2;
pub mod large;
pub mod media;
pub mod tree;
pub mod wiring;

use std::net::SocketAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hyper::body::Bytes;
use sc_ids::hash_block;
use serde_json::{Value, json};
use sqlx::Connection;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::serve_support::account::Account;
use crate::serve_support::http::{Response, call};
use crate::serve_support::{Server, Stack};
use h2::H2;

pub const WITHIN: Duration = Duration::from_secs(10);
/// The largest staged file: one 2 MiB block.
pub const MAX_SMALL: usize = 2 * 1024 * 1024;
/// Concurrent staging streams; below the stack's in-flight block limit (16 sessions x 4 lanes).
const STAGING_STREAMS: usize = 16;
const POLL_EVERY: Duration = Duration::from_millis(200);

/// One small file the client stages under its own key.
pub struct File {
    pub key: Uuid,
    pub name: String,
    pub bytes: Vec<u8>,
}

impl File {
    pub fn new(name: &str, size: usize, seed: u64) -> File {
        File {
            key: Uuid::new_v4(),
            name: name.to_owned(),
            bytes: content(seed, size),
        }
    }
}

/// `size` bytes of a xorshift stream from `seed`, so no two files share content.
pub fn content(seed: u64, size: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let take = (size - bytes.len()).min(8);
        bytes.extend_from_slice(&state.to_le_bytes()[..take]);
    }
    bytes
}

/// `count` files whose sizes run evenly from 0 to 2 MiB, both ends included.
pub fn spread(prefix: &str, count: usize) -> Vec<File> {
    (0..count)
        .map(|index| {
            let size = index * MAX_SMALL / (count - 1);
            let seed = u64::try_from(index).expect("small") + 1;
            File::new(&format!("{prefix}-{index:03}.bin"), size, seed)
        })
        .collect()
}

/// The batch entry of `file` as a new file named `name` under `parent`.
pub fn entry(key: Uuid, parent: Uuid, name: &str) -> Value {
    json!({"staged_key": key, "parent_id": parent, "name": name})
}

pub fn entries(files: &[File], parent: Uuid) -> Vec<Value> {
    files
        .iter()
        .map(|file| entry(file.key, parent, &file.name))
        .collect()
}

pub fn now_micros() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after 1970");
    i64::try_from(since.as_micros()).expect("micros fit i64")
}

/// A node, revision, upload or job id member of a JSON answer.
pub fn uuid_of(value: &Value, member: &str) -> Uuid {
    value[member]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no `{member}` in {value}"))
}

/// The running binary, one account and its device token.
pub struct Pipeline {
    pub stack: Stack,
    pub server: Server,
    pub account: Account,
    pub device: String,
}

impl Pipeline {
    /// The serve stack with `settings` on top of its defaults, the binary ready, and a signed-in device.
    pub async fn start(settings: &[(&str, &str)]) -> Pipeline {
        let mut stack = Stack::new().await;
        for (key, value) in settings {
            stack.root.setup.set(key, value);
        }
        let mut server = stack.spawn();
        server.wait_ready(stack.ports.ops, WITHIN);
        let account = stack.account().await;
        let device = stack.device_token(&account).await;
        Pipeline {
            stack,
            server,
            account,
            device,
        }
    }

    fn public(&self) -> SocketAddr {
        self.stack.ports.public
    }

    /// `method path` as the device.
    pub fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Response {
        call(self.public(), method, path, &self.device, headers, body)
    }

    /// A folder under the account's root, made through `POST /api/v1/nodes`.
    pub fn folder(&self, name: &str) -> Uuid {
        let body = json!({"parent_id": self.account.root, "name": name}).to_string();
        let made = self.call("POST", "/api/v1/nodes", &[json_type()], body.as_bytes());
        assert!((200..300).contains(&made.status), "{made:?}");
        made.json()["node_id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .expect("a node id")
    }

    /// `PUT /api/v1/staged-uploads/{key}` with the file's bytes and, for a non-empty file, its `X-Blake3`.
    pub fn stage(&self, file: &File) -> Response {
        stage_on(self.public(), &self.device, file)
    }

    /// Every file staged in parallel; the responses in file order.
    pub async fn stage_all(&self, files: &[File]) -> Vec<Response> {
        let h2 = H2::connect(self.public()).await;
        let mut running = JoinSet::new();
        let mut answers = Vec::with_capacity(files.len());
        for (index, file) in files.iter().enumerate() {
            if running.len() == STAGING_STREAMS {
                answers.push(joined(running.join_next().await));
            }
            let (h2, device) = (h2.clone(), self.device.clone());
            let (path, headers) = (staged_path(file), staged_headers(file));
            let body = Bytes::from(file.bytes.clone());
            running.spawn(async move {
                let headers: Vec<(&str, String)> =
                    headers.iter().map(|(n, v)| (*n, v.clone())).collect();
                (index, h2.call("PUT", &path, &device, &headers, body).await)
            });
        }
        while let Some(done) = running.join_next().await {
            answers.push(joined(Some(done)));
        }
        answers.sort_by_key(|(index, _)| *index);
        answers.into_iter().map(|(_, answer)| answer).collect()
    }

    /// `POST /api/v1/commits:batch` with `commit_id` and `entries`.
    pub fn commit_batch(&self, commit_id: Uuid, entries: &[Value]) -> Response {
        let body = json!({"commit_id": commit_id, "entries": entries}).to_string();
        self.call(
            "POST",
            "/api/v1/commits:batch",
            &[json_type()],
            body.as_bytes(),
        )
    }

    /// The live children of `folder` (one page of up to 1000).
    pub fn children(&self, folder: Uuid) -> Vec<Value> {
        let listed = self.call(
            "GET",
            &format!("/api/v1/nodes/{folder}/children?limit=1000"),
            &[],
            &[],
        );
        assert_eq!(listed.status, 200, "{listed:?}");
        let listing = listed.json();
        assert!(listing["next"].is_null(), "more than one page: {listing}");
        listing["items"].as_array().expect("items").clone()
    }

    /// The `S` of the account's feed head `E.S`.
    pub fn feed_seq(&self) -> u64 {
        let head = self.call("GET", "/api/v1/changes/head", &[], &[]);
        assert_eq!(head.status, 200, "{head:?}");
        head.json()["cursor"]
            .as_str()
            .and_then(|cursor| cursor.split_once('.'))
            .and_then(|(_, seq)| seq.parse().ok())
            .expect("a cursor `E.S`")
    }

    /// The account's reserved bytes in 07a's ledger.
    pub async fn reserved_bytes(&self) -> i64 {
        let mut admin = self.stack.root.db.admin().await;
        let reserved: i64 =
            sqlx::query_scalar("SELECT reserved_bytes FROM quota WHERE account_id = $1")
                .bind(self.account.id)
                .fetch_one(&mut admin)
                .await
                .expect("the quota row");
        admin.close().await.expect("close admin connection");
        reserved
    }

    /// The staged row of `key`: its reserved bytes and `expires_at` in Unix microseconds; `None` once it is gone.
    pub async fn staged_row(&self, key: Uuid) -> Option<(i64, i64)> {
        let mut admin = self.stack.root.db.admin().await;
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT reserved_bytes, (extract(epoch FROM expires_at) * 1000000)::bigint \
             FROM upload_sessions WHERE account_id = $1 AND client_key = $2",
        )
        .bind(self.account.id)
        .bind(key.to_string())
        .fetch_optional(&mut admin)
        .await
        .expect("read the staged row");
        admin.close().await.expect("close admin connection");
        row
    }

    /// Polls until the wall clock (the binary's clock too) has passed `expires_at`.
    pub async fn wait_past(&self, expires_at: i64) {
        let deadline = Instant::now()
            + Duration::from_micros(u64::try_from(expires_at - now_micros()).unwrap_or(0))
            + WITHIN;
        while now_micros() <= expires_at {
            assert!(
                Instant::now() < deadline,
                "the clock did not pass {expires_at}"
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    /// Polls until the staged row of `key` is gone; panics past `deadline`.
    pub async fn wait_staged_gone(&self, key: Uuid, deadline: Instant) {
        while self.staged_row(key).await.is_some() {
            assert!(
                Instant::now() < deadline,
                "the staged row {key} is still there:\n{}",
                self.server.lines().join("\n")
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    /// Stops the binary (exit 0 on SIGTERM) and drops the database.
    pub async fn finish(mut self) {
        self.server.signal("TERM");
        let exited = self.server.wait_exit(WITHIN);
        assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
        self.stack.finish().await;
    }
}

fn staged_path(file: &File) -> String {
    format!("/api/v1/staged-uploads/{}", file.key)
}

/// The octet-stream type and, for a non-empty file, its `X-Blake3`.
fn staged_headers(file: &File) -> Vec<(&'static str, String)> {
    let mut headers = vec![("Content-Type", "application/octet-stream".to_owned())];
    if !file.bytes.is_empty() {
        headers.push(("X-Blake3", hash_block(&file.bytes).to_string()));
    }
    headers
}

fn joined<T>(done: Option<Result<T, tokio::task::JoinError>>) -> T {
    done.expect("a stream is running")
        .expect("no staging stream panicked")
}

fn stage_on(public: SocketAddr, device: &str, file: &File) -> Response {
    let headers = staged_headers(file);
    call(
        public,
        "PUT",
        &staged_path(file),
        device,
        &headers,
        &file.bytes,
    )
}

fn json_type() -> (&'static str, String) {
    ("Content-Type", "application/json".to_owned())
}
