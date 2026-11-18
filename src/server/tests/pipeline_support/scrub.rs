//! AZ-3667 scrub pipeline: helpers of `pipeline_scrub.rs`. A volume of originals and derived blobs, the corruptions
//! seeded on it from outside (a flipped byte, a removed file), the `scrub run` and `scrub status` commands, the
//! audit events and blob rows the scrub leaves, the admin health route, the scrub series of `/metrics`, and the
//! downloads held open to raise the activity gauge of the API process.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::Value;
use sqlx::Connection;
use uuid::Uuid;

use super::large::LargeFile;
use super::media::jpeg_bytes;
use super::{File, POLL_EVERY, Pipeline, uuid_of};
use crate::serve_support::http::{call, send};

const MIB: usize = 1024 * 1024;
const LANES: usize = 4;

/// One stored file and where its bytes live.
#[derive(Debug, Clone)]
pub struct Stored {
    pub node: Uuid,
    pub revision: Uuid,
    pub blob: Uuid,
    pub bytes: Vec<u8>,
}

/// A `scrub_mismatch` audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub blob: Uuid,
    pub reason: String,
    pub source: String,
}

/// A `scrub run`: the exit code and the `data` of its envelope.
#[derive(Debug)]
pub struct ScrubRun {
    pub code: Option<i32>,
    pub data: Value,
    pub stderr: String,
}

impl ScrubRun {
    pub fn count(&self, key: &str) -> u64 {
        self.data[key]
            .as_u64()
            .unwrap_or_else(|| panic!("no `{key}` in {}\n{}", self.data, self.stderr))
    }
}

impl Pipeline {
    /// A small file published under `parent`.
    pub async fn small(&self, parent: Uuid, name: &str, size: usize, seed: u64) -> Stored {
        let file = File::new(name, size, seed);
        let (node, revision) = self.publish_small(parent, &file, None);
        Stored {
            node,
            revision,
            blob: self.blob_of(revision).await,
            bytes: file.bytes,
        }
    }

    /// A JPEG whose thumb and preview the worker routes then store as derived blobs; the original and the two
    /// derived blob ids.
    pub async fn photo_with_derived(
        &self,
        parent: Uuid,
        name: &str,
        seed: u64,
    ) -> (Stored, [Uuid; 2]) {
        let jpeg = jpeg_bytes(40_000, seed);
        let committed = self.commit_as(&self.device, parent, name, &jpeg, None);
        assert_eq!(committed["status"], "committed", "{committed}");
        let revision = uuid_of(&committed, "revision_id");
        let claimed = self.claim_image("scrub-worker");
        assert_eq!(claimed.status, 200, "{claimed:?}");
        let job = uuid_of(&claimed.json(), "job_id");
        for kind in ["preview", "thumb"] {
            let output = self.put_output("scrub-worker", job, kind, &jpeg_bytes(2000, seed + 100));
            assert_eq!(output.status, 204, "{kind}: {output:?}");
        }
        let derived: Vec<Uuid> = self
            .derived()
            .await
            .into_iter()
            .map(|(_, blob)| blob)
            .collect();
        let derived: [Uuid; 2] = derived
            .try_into()
            .unwrap_or_else(|found| panic!("two derived blobs, found {found:?}"));
        let stored = Stored {
            node: uuid_of(&committed, "node_id"),
            revision,
            blob: self.blob_of(revision).await,
            bytes: jpeg,
        };
        (stored, derived)
    }

    /// A file of `blocks` 2 MiB blocks, uploaded over the F4 routes and committed.
    pub async fn large(&self, parent: Uuid, name: &str, blocks: usize, seed: u64) -> Stored {
        let file = LargeFile::new(name, blocks * 2 * MIB, seed);
        let upload = self.started(parent, &file);
        let wanted: Vec<usize> = (0..file.blocks()).collect();
        self.put_blocks(upload, &file, &wanted, LANES).await;
        let committed = self.commit_upload(upload, parent, &file);
        assert_eq!(committed.status, 200, "{committed:?}");
        let committed = committed.json();
        let revision = uuid_of(&committed, "revision_id");
        Stored {
            node: uuid_of(&committed, "node_id"),
            revision,
            blob: self.blob_of(revision).await,
            bytes: file.bytes,
        }
    }

    /// Flips one byte inside the first sealed chunk of the blob file.
    pub fn corrupt(&self, blob: Uuid) {
        self.flip_chunk_byte(blob, 0);
    }

    /// Removes the blob file.
    pub fn remove_file(&self, blob: Uuid) {
        let path = self.blob_file(blob);
        fs::remove_file(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }

    /// `sunnycloud --json scrub run [--full]`.
    pub fn scrub_run(&self, full: bool) -> ScrubRun {
        let args: &[&str] = if full {
            &["scrub", "run", "--full"]
        } else {
            &["scrub", "run"]
        };
        let run = self.command(args);
        ScrubRun {
            code: run.code,
            data: run.envelope()["data"].clone(),
            stderr: run.stderr,
        }
    }

    /// The `data` of `sunnycloud --json scrub status`.
    pub fn scrub_status(&self) -> Value {
        let run = self.command(&["scrub", "status"]);
        assert_eq!(run.code, Some(0), "{run:?}");
        run.envelope()["data"].clone()
    }

    async fn rows<T>(&self, sql: &'static str, bind: Option<&str>) -> Vec<T>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        let mut admin = self.stack.root.db.admin().await;
        let mut query = sqlx::query_as::<_, T>(sql);
        if let Some(bind) = bind {
            query = query.bind(bind.to_owned());
        }
        let rows = query
            .fetch_all(&mut admin)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
        admin.close().await.expect("close admin connection");
        rows
    }

    /// The `detail` of every audit event of `event`, in order.
    async fn audit_details(&self, event: &'static str) -> Vec<(Option<Uuid>, Value)> {
        let rows: Vec<(Option<Uuid>, Option<String>)> = self
            .rows(
                "SELECT subject_id, detail::text FROM audit_log WHERE event = $1 ORDER BY id",
                Some(event),
            )
            .await;
        rows.into_iter()
            .map(|(subject, detail)| {
                let detail = detail.map_or(Value::Null, |text| {
                    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{text}: {error}"))
                });
                (subject, detail)
            })
            .collect()
    }

    /// Every `scrub_mismatch` event.
    pub async fn findings(&self) -> Vec<Finding> {
        self.audit_details("scrub_mismatch")
            .await
            .into_iter()
            .map(|(subject, detail)| {
                let text = |key: &str| detail[key].as_str().unwrap_or_default().to_owned();
                Finding {
                    blob: subject
                        .unwrap_or_else(|| panic!("a finding without a subject: {detail}")),
                    reason: text("reason"),
                    source: text("source"),
                }
            })
            .collect()
    }

    /// The blob and `reason/source` of every `scrub_mismatch` event, in blob order.
    pub async fn finding_pairs(&self) -> Vec<(Uuid, String)> {
        let mut pairs: Vec<(Uuid, String)> = self
            .findings()
            .await
            .into_iter()
            .map(|f| (f.blob, format!("{}/{}", f.reason, f.source)))
            .collect();
        pairs.sort();
        pairs
    }

    /// How many of `blobs` have a row, a revision, a `blob_gc` row or a file left.
    pub async fn not_collected(&self, blobs: &[Uuid]) -> usize {
        let mut left = 0;
        for blob in blobs {
            left += usize::from(self.stored(*blob).await != super::gc::Stored::GONE);
        }
        left
    }

    /// `count` uploads of 6 MiB files started under `parent` with blocks 0 and 1 sent, so their blobs are in
    /// `uploading`; each with the state of its blob.
    pub async fn inflight(
        &self,
        parent: Uuid,
        count: u64,
    ) -> Vec<(Uuid, LargeFile, Option<String>)> {
        let mut sessions = Vec::new();
        for index in 0..count {
            let file = LargeFile::new(&format!("inflight{index}.bin"), 3 * 2 * MIB, 40 + index);
            let upload = self.started(parent, &file);
            self.put_blocks(upload, &file, &[0, 1], 2).await;
            let blob = self.session_blob(upload).await.expect("session row");
            sessions.push((upload, file, self.blob_state(blob).await));
        }
        sessions
    }

    /// Sends the last block of an [`inflight`](Self::inflight) upload and commits it; its node.
    pub async fn finish_inflight(&self, parent: Uuid, upload: Uuid, file: &LargeFile) -> Uuid {
        self.put_blocks(upload, file, &[2], 1).await;
        let committed = self.commit_upload(upload, parent, file);
        assert_eq!(committed.status, 200, "{committed:?}");
        uuid_of(&committed.json(), "node_id")
    }

    /// The `detail` of every `scrub_summary` event.
    pub async fn summaries(&self) -> Vec<Value> {
        self.audit_details("scrub_summary")
            .await
            .into_iter()
            .map(|(_, detail)| detail)
            .collect()
    }

    /// The committed blobs, in id order.
    pub async fn committed_blobs(&self) -> Vec<Uuid> {
        self.rows::<(Uuid,)>(
            "SELECT id FROM blobs WHERE state = 'committed' ORDER BY id",
            None,
        )
        .await
        .into_iter()
        .map(|(id,)| id)
        .collect()
    }

    /// The blobs whose row carries `unavailable_at`, in id order.
    pub async fn unavailable_blobs(&self) -> Vec<Uuid> {
        self.rows::<(Uuid,)>(
            "SELECT id FROM blobs WHERE unavailable_at IS NOT NULL ORDER BY id",
            None,
        )
        .await
        .into_iter()
        .map(|(id,)| id)
        .collect()
    }

    /// Committed blobs no scrub has checked yet.
    pub async fn never_scrubbed(&self) -> i64 {
        let rows: Vec<(i64,)> = self
            .rows(
                "SELECT count(*) FROM blobs WHERE state = 'committed' AND last_scrubbed_at IS NULL",
                None,
            )
            .await;
        rows[0].0
    }

    /// Makes the pipeline's account an admin (superuser SQL, as an operator's first-admin setup does).
    pub async fn make_admin(&self) {
        let mut admin = self.stack.root.db.admin().await;
        sqlx::query("UPDATE accounts SET role = 'admin' WHERE id = $1")
            .bind(self.account.id)
            .execute(&mut admin)
            .await
            .expect("promote the account");
        admin.close().await.expect("close admin connection");
    }

    /// The status and detail of the blobstore item of `GET /api/v1/admin/health` (07's aggregation of the
    /// registered `HealthContributor`s), read as the admin session.
    pub fn blobstore_health(&self) -> (String, String) {
        let answer = call(
            self.public(),
            "GET",
            "/api/v1/admin/health",
            &self.account.token,
            &[],
            &[],
        );
        assert_eq!(answer.status, 200, "{answer:?}");
        let body = answer.json();
        let item = body["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == "blobstore"))
            .unwrap_or_else(|| panic!("no blobstore item in {body}"));
        let text = |key: &str| item[key].as_str().unwrap_or_default().to_owned();
        (text("status"), text("detail"))
    }

    /// The value of the unlabelled `sunnycloud_<name>` series of the ops listener; panics when it is not there.
    pub fn metric(&self, name: &str) -> f64 {
        let scrape = send(self.stack.ports.ops, "GET", "/metrics", &[], b"");
        assert_eq!(scrape.status, 200, "{scrape:?}");
        let text = String::from_utf8_lossy(&scrape.body).into_owned();
        let series = format!("sunnycloud_{name} ");
        text.lines()
            .find_map(|line| line.strip_prefix(&series)?.trim().parse::<f64>().ok())
            .unwrap_or_else(|| panic!("no `{series}` in the scrape:\n{text}"))
    }

    /// `GET` of the node's content with the response head read and the body left unread, so the server's body
    /// stream, and the activity gauge it counts in, stays open until the returned socket is dropped.
    pub fn hold_download(&self, node: Uuid) -> TcpStream {
        let mut stream = TcpStream::connect(self.public()).expect("tcp connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("read timeout");
        let request = format!(
            "GET /api/v1/nodes/{node}/content HTTP/1.1\r\nHost: cloud.example.org\r\n\
             Authorization: Bearer {}\r\n\r\n",
            self.device
        );
        stream.write_all(request.as_bytes()).expect("the request");
        let mut head = Vec::new();
        let mut buffer = [0u8; 1024];
        while !head.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).expect("the response head");
            assert!(read > 0, "the connection closed before the head");
            head.extend_from_slice(&buffer[..read]);
        }
        let text = String::from_utf8_lossy(&head);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        stream
    }

    /// Polls `done` every 200 ms until it holds; panics with `what` and the binary's log past `deadline`.
    pub async fn poll<F>(&self, what: &str, deadline: Instant, mut done: F)
    where
        F: AsyncFnMut() -> bool,
    {
        while !done().await {
            assert!(
                Instant::now() < deadline,
                "{what}:\n{}",
                self.server.lines().join("\n")
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }
}
