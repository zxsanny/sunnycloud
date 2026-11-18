//! Flow F12 on the [`Pipeline`] (AZ-3663): the test clock of a `test-clock` build, the janitor cycle counters of
//! the ops listener, superuser probes of what the GC removes (revision, blob and `blob_gc` rows, the blob file,
//! `derived` rows, tombstones and the purge horizon), a revision row lock held from outside, a worker output
//! upload, and the RFC 3339 times `admin restore --at` takes.

use std::path::PathBuf;
use std::time::Instant;

use chrono::{DateTime, SecondsFormat};
use serde_json::json;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use super::media::WORKER_TOKEN;
use super::{POLL_EVERY, Pipeline, json_type, now_micros};
use crate::serve_support::http::{Response, call, janitor_cycles, send};

pub const DAY: u64 = 86_400;

/// What is stored of one blob: the revision row pointing at it, its blob row, its `blob_gc` row, its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    pub revision: bool,
    pub blob: bool,
    pub gc: bool,
    pub file: bool,
}

impl Stored {
    /// A deleted or replaced original inside retention: every row and the file.
    pub const QUEUED: Stored = Stored {
        revision: true,
        blob: true,
        gc: true,
        file: true,
    };
    /// A collected blob.
    pub const GONE: Stored = Stored {
        revision: false,
        blob: false,
        gc: false,
        file: false,
    };
}

/// Unix microseconds as the RFC 3339 UTC text of `admin restore --at`.
pub fn rfc3339(micros: i64) -> String {
    DateTime::from_timestamp_micros(micros)
        .expect("a representable time")
        .to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Releases a lock taken by [`Pipeline::lock_revision`].
pub async fn release(mut held: PgConnection) {
    sqlx::query("ROLLBACK")
        .execute(&mut held)
        .await
        .expect("release the lock");
    held.close().await.expect("close the locking connection");
}

impl Pipeline {
    /// `POST /test/clock` advancing the binary's clock by `seconds`; the new offset in seconds.
    pub fn advance_clock(&self, seconds: u64) -> u64 {
        let body = json!({"advance_seconds": seconds}).to_string();
        let moved = send(
            self.stack.ports.test_clock,
            "POST",
            "/test/clock",
            &[json_type()],
            body.as_bytes(),
        );
        assert_eq!(moved.status, 200, "{moved:?}");
        moved.json()["offset_seconds"].as_u64().expect("an offset")
    }

    /// Finished cycles of the janitor `task` so far.
    pub fn janitor_cycles(&self, task: &str) -> u64 {
        janitor_cycles(self.stack.ports.ops, task)
    }

    /// Polls until `count` more cycles of `task` have finished ok; panics past `deadline`.
    pub async fn wait_cycles(&self, task: &str, count: u64, deadline: Instant) {
        let target = self.janitor_cycles(task) + count;
        while self.janitor_cycles(task) < target {
            assert!(
                Instant::now() < deadline,
                "{task} did not finish {count} cycles"
            );
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    /// Runs `sql` with `$1` bound to `id` and returns its single `bigint`.
    async fn count_of(&self, sql: &'static str, id: Uuid) -> i64 {
        let mut admin = self.stack.root.db.admin().await;
        let count: i64 = sqlx::query_scalar(sql)
            .bind(id)
            .fetch_one(&mut admin)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
        admin.close().await.expect("close admin connection");
        count
    }

    /// What is stored of `blob`.
    pub async fn stored(&self, blob: Uuid) -> Stored {
        Stored {
            revision: self
                .count_of("SELECT count(*) FROM revisions WHERE blob_id = $1", blob)
                .await
                > 0,
            blob: self
                .count_of("SELECT count(*) FROM blobs WHERE id = $1", blob)
                .await
                > 0,
            gc: self
                .count_of("SELECT count(*) FROM blob_gc WHERE blob_id = $1", blob)
                .await
                > 0,
            file: self.blob_file(blob).exists(),
        }
    }

    /// Polls until every blob of `blobs` is stored as `want`; panics past `deadline`.
    pub async fn wait_stored(&self, blobs: &[Uuid], want: Stored, deadline: Instant) {
        for &blob in blobs {
            loop {
                let found = self.stored(blob).await;
                if found == want {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "blob {blob} is {found:?}, not {want:?}:\n{}",
                    self.server.lines().join("\n")
                );
                tokio::time::sleep(POLL_EVERY).await;
            }
        }
    }

    /// `<blob file>.gc`, where 04's sweeper parks a file before its rows are removed.
    pub fn parked_file(&self, blob: Uuid) -> PathBuf {
        let mut path = self.blob_file(blob).into_os_string();
        path.push(".gc");
        PathBuf::from(path)
    }

    /// The `(kind, blob)` of the account's `derived` rows, by kind.
    pub async fn derived(&self) -> Vec<(String, Uuid)> {
        let mut admin = self.stack.root.db.admin().await;
        let rows: Vec<(String, Uuid)> =
            sqlx::query_as("SELECT kind, blob_id FROM derived WHERE account_id = $1 ORDER BY kind")
                .bind(self.account.id)
                .fetch_all(&mut admin)
                .await
                .expect("read the derived rows");
        admin.close().await.expect("close admin connection");
        rows
    }

    /// Whether the node row exists and, if so, whether it is a tombstone.
    pub async fn tombstone(&self, node: Uuid) -> Option<bool> {
        let mut admin = self.stack.root.db.admin().await;
        let deleted: Option<bool> =
            sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM nodes WHERE id = $1")
                .bind(node)
                .fetch_optional(&mut admin)
                .await
                .expect("read the node row");
        admin.close().await.expect("close admin connection");
        deleted
    }

    /// The account's feed purge horizon (a seq).
    pub async fn purge_horizon(&self) -> i64 {
        let mut admin = self.stack.root.db.admin().await;
        let horizon: i64 =
            sqlx::query_scalar("SELECT purge_horizon FROM account_feeds WHERE account_id = $1")
                .bind(self.account.id)
                .fetch_one(&mut admin)
                .await
                .expect("read the feed row");
        admin.close().await.expect("close admin connection");
        horizon
    }

    /// Server backends of this database waiting for a lock in a `DELETE FROM revisions` (02's owner cleanup).
    pub async fn revision_deletes_waiting(&self) -> i64 {
        let mut admin = self.stack.root.db.admin().await;
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
             AND wait_event_type = 'Lock' AND query LIKE 'DELETE FROM revisions%'",
        )
        .fetch_one(&mut admin)
        .await
        .expect("read pg_stat_activity");
        admin.close().await.expect("close admin connection");
        waiting
    }

    /// A superuser transaction holding `FOR UPDATE` on the revision row, open until [`release`].
    pub async fn lock_revision(&self, revision: Uuid) -> PgConnection {
        let mut held = self.stack.root.db.admin().await;
        sqlx::query("BEGIN")
            .execute(&mut held)
            .await
            .expect("begin");
        let locked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM (SELECT id FROM revisions WHERE id = $1 FOR UPDATE) AS locked",
        )
        .bind(revision)
        .fetch_one(&mut held)
        .await
        .expect("lock the revision row");
        assert_eq!(locked, 1, "no revision {revision} to lock");
        held
    }

    /// `PUT /api/v1/worker-jobs/{job}/outputs/{kind}` of `bytes` as worker `worker`.
    pub fn put_output(&self, worker: &str, job: Uuid, kind: &str, bytes: &[u8]) -> Response {
        let headers = [
            ("Content-Type", "image/jpeg".to_owned()),
            ("X-Worker-Id", worker.to_owned()),
        ];
        let path = format!("/api/v1/worker-jobs/{job}/outputs/{kind}");
        call(
            self.stack.ports.internal,
            "PUT",
            &path,
            WORKER_TOKEN,
            &headers,
            bytes,
        )
    }

    /// The JSON log lines of the running binary at level `error`.
    pub fn error_lines(&self) -> Vec<String> {
        self.server
            .lines()
            .into_iter()
            .filter(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .is_ok_and(|value| value["level"] == "error")
            })
            .collect()
    }

    /// The wall clock now, as `admin restore --at` takes it.
    pub fn now_rfc3339(&self) -> String {
        rfc3339(now_micros())
    }
}
