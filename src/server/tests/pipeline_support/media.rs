//! Flow F10 steps 1-3 on the [`Pipeline`], the API side: the test calls the worker listener in the supervisor's
//! place with the configured worker bearer (`worker_api_token` of the serve harness), streams a job's input and
//! hashes it block by block as the supervisor does, reads the job rows and runs `sunnycloud admin media-stats`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sc_ids::{BLOCK_SIZE, FileHash, FileHasher, Hash32};
use serde_json::Value;
use sqlx::Connection;
use uuid::Uuid;

use super::{POLL_EVERY, Pipeline, json_type};
use crate::serve_support::http::{Response, call};

/// `worker_api_token` as `config_support::Setup::valid` writes it.
pub const WORKER_TOKEN: &str = "worker-token";
const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// A JPEG small file: the JPEG signature (`FF D8 FF E0`) then `size - 4` bytes of `seed`'s stream.
pub fn jpeg_bytes(size: usize, seed: u64) -> Vec<u8> {
    let mut bytes = super::content(seed, size);
    bytes[..4].copy_from_slice(&[0xff, 0xd8, 0xff, 0xe0]);
    bytes
}

/// A job row: status, attempts, outcome, priority and last error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub status: String,
    pub attempts: i16,
    pub outcome: Option<String>,
    pub priority: i16,
    pub last_error: Option<String>,
}

/// `kind, status, attempts, outcome, priority, last_error` of a job.
type KindRow = (String, String, i16, Option<String>, i16, Option<String>);

/// A job input read to its end: the status, the `Content-Length`, the 2 MiB block hashes and their roll-up; the
/// body of an answer other than 200.
pub struct Input {
    pub status: u16,
    pub length: Option<u64>,
    pub blocks: Vec<Hash32>,
    pub file_hash: Option<FileHash>,
    pub body: Vec<u8>,
}

impl Input {
    pub fn code(&self) -> String {
        serde_json::from_slice::<Value>(&self.body)
            .ok()
            .and_then(|body| body["code"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

impl Pipeline {
    /// `method path` on the internal listener with `token` as the bearer and `worker` as `X-Worker-Id`.
    pub fn worker_call(
        &self,
        token: &str,
        worker: &str,
        method: &str,
        path: &str,
        body: &str,
    ) -> Response {
        let headers = [json_type(), ("X-Worker-Id", worker.to_owned())];
        let addr = self.stack.ports.internal;
        call(addr, method, path, token, &headers, body.as_bytes())
    }

    /// `POST /api/v1/worker-jobs/claims` of the image lane as worker `worker`.
    pub fn claim_image(&self, worker: &str) -> Response {
        let path = "/api/v1/worker-jobs/claims";
        self.worker_call(WORKER_TOKEN, worker, "POST", path, r#"{"lane":"image"}"#)
    }

    /// `PUT /api/v1/worker-jobs/{job}/lease` as worker `worker`.
    pub fn extend_lease(&self, worker: &str, job: Uuid) -> Response {
        let path = format!("/api/v1/worker-jobs/{job}/lease");
        self.worker_call(WORKER_TOKEN, worker, "PUT", &path, "")
    }

    /// `GET /api/v1/worker-jobs/{job}/input` as worker `worker`, streamed and hashed per 2 MiB block.
    pub fn job_input(&self, worker: &str, job: Uuid) -> Input {
        let mut stream =
            TcpStream::connect(self.stack.ports.internal).expect("connect to the worker listener");
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .expect("read timeout");
        let head = format!(
            "GET /api/v1/worker-jobs/{job}/input HTTP/1.1\r\nHost: api\r\nConnection: close\r\n\
             Authorization: Bearer {WORKER_TOKEN}\r\nX-Worker-Id: {worker}\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).expect("the request");
        let mut reader = BufReader::new(stream);
        let (status, length) = response_head(&mut reader);
        if status != 200 {
            let mut body = Vec::new();
            reader.read_to_end(&mut body).expect("the error body");
            return Input {
                status,
                length,
                blocks: Vec::new(),
                file_hash: None,
                body,
            };
        }
        let total = length.expect("a 200 input carries its Content-Length");
        let (blocks, file_hash) = hash_blocks(&mut reader, total);
        Input {
            status,
            length,
            blocks,
            file_hash: Some(file_hash),
            body: Vec::new(),
        }
    }

    /// The job rows of `revision`, `(kind, row)` by kind.
    pub async fn jobs_of(&self, revision: Uuid) -> Vec<(String, JobRow)> {
        let mut admin = self.stack.root.db.admin().await;
        let rows: Vec<KindRow> = sqlx::query_as(
            "SELECT kind, status, attempts, outcome, priority, last_error FROM jobs \
             WHERE revision_id = $1 ORDER BY kind",
        )
        .bind(revision)
        .fetch_all(&mut admin)
        .await
        .expect("read the jobs");
        admin.close().await.expect("close admin connection");
        rows.into_iter()
            .map(|(kind, status, attempts, outcome, priority, last_error)| {
                let row = JobRow {
                    status,
                    attempts,
                    outcome,
                    priority,
                    last_error,
                };
                (kind, row)
            })
            .collect()
    }

    /// The ids of the jobs of `revision`.
    pub async fn job_ids_of(&self, revision: Uuid) -> Vec<Uuid> {
        let mut admin = self.stack.root.db.admin().await;
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM jobs WHERE revision_id = $1")
            .bind(revision)
            .fetch_all(&mut admin)
            .await
            .expect("read the job ids");
        admin.close().await.expect("close admin connection");
        ids
    }

    /// The row of `job` and its lease end (`available_at`) in Unix microseconds; `None` once it is gone.
    pub async fn job(&self, job: Uuid) -> Option<(JobRow, i64)> {
        let mut admin = self.stack.root.db.admin().await;
        type Row = (String, i16, Option<String>, i16, Option<String>, i64);
        let row: Option<Row> = sqlx::query_as(
            "SELECT status, attempts, outcome, priority, last_error, \
             (extract(epoch FROM available_at) * 1000000)::bigint FROM jobs WHERE id = $1",
        )
        .bind(job)
        .fetch_optional(&mut admin)
        .await
        .expect("read the job");
        admin.close().await.expect("close admin connection");
        row.map(|(status, attempts, outcome, priority, last_error, until)| {
            let row = JobRow {
                status,
                attempts,
                outcome,
                priority,
                last_error,
            };
            (row, until)
        })
    }

    /// Polls until `job` has `status`; panics past `deadline`.
    pub async fn wait_job_status(&self, job: Uuid, status: &str, deadline: Instant) -> JobRow {
        loop {
            let row = self.job(job).await.map(|(row, _)| row);
            if let Some(row) = row.as_ref().filter(|row| row.status == status) {
                return row.clone();
            }
            assert!(
                Instant::now() < deadline,
                "job {job} is {row:?}, not {status}:\n{}",
                self.server.lines().join("\n")
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    /// Polls until the revision row is gone (collected by the GC loop); panics past `deadline`.
    pub async fn wait_revision_collected(&self, revision: Uuid, deadline: Instant) {
        loop {
            let mut admin = self.stack.root.db.admin().await;
            let left: i64 = sqlx::query_scalar("SELECT count(*) FROM revisions WHERE id = $1")
                .bind(revision)
                .fetch_one(&mut admin)
                .await
                .expect("read the revision");
            admin.close().await.expect("close admin connection");
            if left == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "revision {revision} is still stored:\n{}",
                self.server.lines().join("\n")
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    /// The `data` of `sunnycloud --json admin media-stats` on this stack.
    pub fn media_stats(&self) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
            .args(["--json", "admin", "media-stats"])
            .env_clear()
            .envs(&self.stack.root.setup.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("media-stats runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
            panic!(
                "media-stats printed no JSON envelope ({error}): {stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(output.status.code(), Some(0), "{envelope}");
        envelope["data"].clone()
    }
}

/// The status and `Content-Length` of the response head, read line by line.
fn response_head(reader: &mut BufReader<TcpStream>) -> (u16, Option<u64>) {
    let mut line = String::new();
    reader.read_line(&mut line).expect("the status line");
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in {line:?}"));
    let mut length = None;
    loop {
        line.clear();
        reader.read_line(&mut line).expect("a header line");
        let header = line.trim_end();
        if header.is_empty() {
            return (status, length);
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok();
        }
    }
}

/// Reads exactly `total` bytes in 2 MiB blocks, each hashed and rolled up as the supervisor verifies its input.
fn hash_blocks(reader: &mut BufReader<TcpStream>, total: u64) -> (Vec<Hash32>, FileHash) {
    let mut hasher = FileHasher::new();
    let mut block = vec![0u8; BLOCK_SIZE];
    let mut left = total;
    while left > 0 {
        let take = usize::try_from(left.min(BLOCK_SIZE as u64)).expect("a block fits usize");
        reader
            .read_exact(&mut block[..take])
            .unwrap_or_else(|error| panic!("{} of {total} bytes read: {error}", total - left));
        hasher
            .update(&block[..take])
            .expect("a block of at most 2 MiB");
        hasher.finish_block();
        left -= take as u64;
    }
    let mut extra = [0u8; 1];
    let after = reader.read(&mut extra).expect("the end of the body");
    assert_eq!(after, 0, "more bytes than the Content-Length");
    hasher.finish()
}
