//! Flow F4 on the [`Pipeline`]: a file of 2 MiB blocks, the upload-session routes (start, block PUT over up to four
//! h2c lanes, state, commit), the content download, a crash of the binary and its restart on the same database and
//! volume, and probes of the session, blob, revision and job rows and of the blob files.

use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Instant;

use hyper::body::Bytes;
use sc_ids::{BLOCK_SIZE, hash_block};
use serde_json::{Value, json};
use sqlx::Connection;
use tokio::task::JoinSet;
use uuid::Uuid;

use super::h2::H2;
use super::{POLL_EVERY, Pipeline, WITHIN, content, json_type};
use crate::serve_support::http::Response;

/// A file sent as 2 MiB blocks.
pub struct LargeFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl LargeFile {
    pub fn new(name: &str, size: usize, seed: u64) -> LargeFile {
        LargeFile {
            name: name.to_owned(),
            bytes: content(seed, size),
        }
    }

    /// A file whose head is a JPEG signature (`FF D8 FF E0`), so 03 sniffs `image/jpeg`.
    pub fn jpeg(name: &str, size: usize, seed: u64) -> LargeFile {
        let mut file = LargeFile::new(name, size, seed);
        file.bytes[..4].copy_from_slice(&[0xff, 0xd8, 0xff, 0xe0]);
        file
    }

    pub fn blocks(&self) -> usize {
        self.bytes.len().div_ceil(BLOCK_SIZE)
    }

    pub fn block(&self, index: usize) -> &[u8] {
        let end = ((index + 1) * BLOCK_SIZE).min(self.bytes.len());
        &self.bytes[index * BLOCK_SIZE..end]
    }

    pub fn blocklist(&self) -> Vec<String> {
        (0..self.blocks())
            .map(|index| hash_block(self.block(index)).to_string())
            .collect()
    }
}

pub fn indexes(value: &Value) -> Vec<usize> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("an index list: {value}"))
        .iter()
        .map(|index| usize::try_from(index.as_u64().expect("an index")).expect("small"))
        .collect()
}

fn id_of(response: &Response, member: &str) -> Uuid {
    response.json()[member]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no `{member}` in {response:?}"))
}

impl Pipeline {
    /// `POST /api/v1/uploads` for `file` named `name` under `parent`.
    pub fn start_upload(&self, parent: Uuid, file: &LargeFile, name: &str) -> Response {
        let body = json!({
            "parent_id": parent,
            "name": name,
            "declared_size": file.bytes.len(),
            "client_key": format!("{name}-{}", Uuid::new_v4().simple()),
        });
        self.call(
            "POST",
            "/api/v1/uploads",
            &[json_type()],
            body.to_string().as_bytes(),
        )
    }

    /// A started session's id; panics unless the start answered `session`.
    pub fn started(&self, parent: Uuid, file: &LargeFile) -> Uuid {
        let started = self.start_upload(parent, file, &file.name);
        assert_eq!(started.status, 200, "{started:?}");
        assert_eq!(started.json()["outcome"], "session", "{started:?}");
        id_of(&started, "upload_id")
    }

    /// `PUT` of block `index` with `hash` as its `X-Blake3`.
    pub fn put_block_hashed(
        &self,
        upload: Uuid,
        file: &LargeFile,
        index: usize,
        hash: &str,
    ) -> Response {
        self.call(
            "PUT",
            &format!("/api/v1/uploads/{upload}/blocks/{index}"),
            &block_headers(hash),
            file.block(index),
        )
    }

    pub fn put_block(&self, upload: Uuid, file: &LargeFile, index: usize) -> Response {
        let hash = hash_block(file.block(index)).to_string();
        self.put_block_hashed(upload, file, index, &hash)
    }

    /// The blocks `wanted` over `lanes` concurrent streams of one h2c connection, lane `k` sending every
    /// `lanes`-th block in order; every block must be acknowledged with 204.
    pub async fn put_blocks(&self, upload: Uuid, file: &LargeFile, wanted: &[usize], lanes: usize) {
        let h2 = H2::connect(self.public()).await;
        let mut running = JoinSet::new();
        for lane in 0..lanes {
            let blocks: Vec<(usize, Bytes)> = wanted
                .iter()
                .skip(lane)
                .step_by(lanes)
                .map(|&index| (index, Bytes::copy_from_slice(file.block(index))))
                .collect();
            let (h2, device) = (h2.clone(), self.device.clone());
            running.spawn(async move {
                for (index, block) in blocks {
                    let headers = block_headers(&hash_block(&block).to_string());
                    let path = format!("/api/v1/uploads/{upload}/blocks/{index}");
                    let acked = h2.call("PUT", &path, &device, &headers, block).await;
                    assert_eq!(acked.status, 204, "block {index}: {acked:?}");
                }
            });
        }
        while let Some(lane) = running.join_next().await {
            lane.expect("a lane ends without a panic");
        }
    }

    /// Sends the head and half of block `index`, then drops the connection: the block is never acknowledged.
    pub fn drop_block_midway(&self, upload: Uuid, file: &LargeFile, index: usize) {
        let block = file.block(index);
        let mut stream = self.raw_request(
            "PUT",
            &format!("/api/v1/uploads/{upload}/blocks/{index}"),
            &block_headers(&hash_block(block).to_string()),
            block.len(),
        );
        stream
            .write_all(&block[..block.len() / 2])
            .expect("half a block");
        drop(stream);
    }

    /// Writes a whole request and returns the open connection without reading the answer.
    pub fn send_unanswered(&self, method: &str, path: &str, body: &[u8]) -> TcpStream {
        let mut stream = self.raw_request(method, path, &[json_type()], body.len());
        stream.write_all(body).expect("the request body");
        stream.flush().expect("flush the request");
        stream
    }

    fn raw_request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        length: usize,
    ) -> TcpStream {
        let mut stream = TcpStream::connect(self.public()).expect("tcp connect");
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: cloud.example.org\r\nAuthorization: Bearer {}\r\n\
             Content-Length: {length}\r\n",
            self.device
        );
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).expect("the request head");
        stream
    }

    /// `GET /api/v1/uploads/{upload}`.
    pub fn upload_state(&self, upload: Uuid) -> Response {
        self.call("GET", &format!("/api/v1/uploads/{upload}"), &[], &[])
    }

    pub fn commit_body(upload: Uuid, parent: Uuid, file: &LargeFile) -> Vec<u8> {
        json!({
            "upload_id": upload,
            "parent_id": parent,
            "name": file.name,
            "blocklist": file.blocklist(),
        })
        .to_string()
        .into_bytes()
    }

    /// `POST /api/v1/commits` of the whole blocklist of `file`.
    pub fn commit_upload(&self, upload: Uuid, parent: Uuid, file: &LargeFile) -> Response {
        let body = Pipeline::commit_body(upload, parent, file);
        self.call("POST", "/api/v1/commits", &[json_type()], &body)
    }

    pub fn node(&self, node: Uuid) -> Value {
        let found = self.call("GET", &format!("/api/v1/nodes/{node}"), &[], &[]);
        assert_eq!(found.status, 200, "{found:?}");
        found.json()
    }

    /// The plaintext of the node's current revision.
    pub fn download(&self, node: Uuid) -> Vec<u8> {
        let content = self.call("GET", &format!("/api/v1/nodes/{node}/content"), &[], &[]);
        assert_eq!(content.status, 200, "{content:?}");
        content.body
    }

    /// The account's feed cursor `E.S`.
    pub fn feed_cursor(&self) -> String {
        let head = self.call("GET", "/api/v1/changes/head", &[], &[]);
        assert_eq!(head.status, 200, "{head:?}");
        head.json()["cursor"].as_str().expect("a cursor").to_owned()
    }

    /// SIGKILL of the binary, then a start on the same database and blob volume.
    pub fn crash_and_restart(&mut self) {
        self.server.crash();
        self.server = self.stack.spawn();
        self.server.wait_ready(self.stack.ports.ops, WITHIN);
    }

    /// Runs `sql` with `$1` bound to the account and returns its single `bigint`.
    async fn account_count(&self, sql: &'static str) -> i64 {
        let mut admin = self.stack.root.db.admin().await;
        let count: i64 = sqlx::query_scalar(sql)
            .bind(self.account.id)
            .fetch_one(&mut admin)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
        admin.close().await.expect("close admin connection");
        count
    }

    /// The blob of the session `upload`; `None` once the session row is gone.
    pub async fn session_blob(&self, upload: Uuid) -> Option<Uuid> {
        let mut admin = self.stack.root.db.admin().await;
        let blob: Option<Uuid> =
            sqlx::query_scalar("SELECT blob_id FROM upload_sessions WHERE id = $1")
                .bind(upload)
                .fetch_optional(&mut admin)
                .await
                .expect("read the session row");
        admin.close().await.expect("close admin connection");
        blob
    }

    /// The state of the blob row; `None` once it is gone.
    pub async fn blob_state(&self, blob: Uuid) -> Option<String> {
        let mut admin = self.stack.root.db.admin().await;
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM blobs WHERE id = $1")
            .bind(blob)
            .fetch_optional(&mut admin)
            .await
            .expect("read the blob row");
        admin.close().await.expect("close admin connection");
        state
    }

    /// `<blob_root>/blobs/<id[0:2]>/<id[2:4]>/<id>`, 04's layout.
    pub fn blob_file(&self, blob: Uuid) -> PathBuf {
        let text = blob.to_string();
        self.stack
            .blob_root()
            .join("blobs")
            .join(&text[0..2])
            .join(&text[2..4])
            .join(&text)
    }

    /// The account's revisions whose blob row is missing or not committed, or whose blob file is missing.
    pub async fn dangling_revisions(&self) -> Vec<String> {
        let mut admin = self.stack.root.db.admin().await;
        let rows: Vec<(Uuid, Uuid, Option<String>)> = sqlx::query_as(
            "SELECT r.id, r.blob_id, b.state FROM revisions r LEFT JOIN blobs b ON b.id = r.blob_id \
             WHERE r.account_id = $1",
        )
        .bind(self.account.id)
        .fetch_all(&mut admin)
        .await
        .expect("read the revisions");
        admin.close().await.expect("close admin connection");
        rows.into_iter()
            .filter(|(_, blob, state)| {
                state.as_deref() != Some("committed") || !self.blob_file(*blob).is_file()
            })
            .map(|(revision, blob, state)| format!("revision {revision}: blob {blob} {state:?}"))
            .collect()
    }

    pub async fn revision_count(&self) -> i64 {
        self.account_count("SELECT count(*) FROM revisions WHERE account_id = $1")
            .await
    }

    pub async fn job_count(&self) -> i64 {
        self.account_count("SELECT count(*) FROM jobs WHERE account_id = $1")
            .await
    }

    /// The media jobs of `revision`, by kind.
    pub async fn job_kinds(&self, revision: Uuid) -> Vec<String> {
        let mut admin = self.stack.root.db.admin().await;
        let kinds: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM jobs WHERE revision_id = $1 ORDER BY kind")
                .bind(revision)
                .fetch_all(&mut admin)
                .await
                .expect("read the jobs");
        admin.close().await.expect("close admin connection");
        kinds
    }

    /// Superuser SQL on the account's row of `table` (arranged from outside, as the serve harness does).
    pub async fn arrange(&self, sql: &'static str) {
        let mut admin = self.stack.root.db.admin().await;
        sqlx::query(sql)
            .bind(self.account.id)
            .execute(&mut admin)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
        admin.close().await.expect("close admin connection");
    }

    /// Polls until the session row of `upload` is gone; panics past `deadline`.
    pub async fn wait_session_gone(&self, upload: Uuid, deadline: Instant) {
        while self.session_blob(upload).await.is_some() {
            assert!(
                Instant::now() < deadline,
                "the session {upload} is still there:\n{}",
                self.server.lines().join("\n")
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    /// Polls until the blob file is gone; panics past `deadline`.
    pub async fn wait_file_gone(&self, blob: Uuid, deadline: Instant) {
        while self.blob_file(blob).exists() {
            assert!(
                Instant::now() < deadline,
                "the blob file {blob} is still there:\n{}",
                self.server.lines().join("\n")
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }
}

fn block_headers(hash: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Content-Type", "application/octet-stream".to_owned()),
        ("X-Blake3", hash.to_owned()),
    ]
}
