//! A job queue over a per-test database with a manual clock. Jobs are added through `enqueue_in_tx` in the
//! account's scope; rows are read as the schema owner. Every row read checks that `status`, `finished_at` and
//! `outcome` agree, so each test also pins that invariant.

use std::sync::Arc;
use std::time::Duration;

use media_api::{EnqueueOutcome, JobMetrics, JobQueue, MediaConfig, Priority};
use media_shared::policy::JobSpec;
use media_shared::types::JobKind;
use sc_clock::{Clock, ManualClock};
use sc_db::{DbError, Scope};
use sc_ids::FileHash;
use sc_observability::{BuildInfo, Metrics};
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{Fixture, START, account_with_revision, load_settings};

pub const LEASE: Duration = Duration::from_secs(2);
pub const BACKOFF: Duration = Duration::from_millis(200);
pub const ATTEMPTS: u64 = 3;

pub struct Queue {
    pub fixture: Fixture,
    pub queue: Arc<JobQueue>,
    pub clock: Arc<ManualClock>,
    pub metrics: Arc<JobMetrics>,
    pub account: Uuid,
    pub revision: Uuid,
}

#[derive(Debug, sqlx::FromRow)]
pub struct Row {
    pub status: String,
    pub attempts: i32,
    pub priority: i32,
    pub leased_by: Option<String>,
    pub outcome: Option<String>,
    pub last_error: Option<String>,
    pub available_at: i64,
    pub finished_at: Option<i64>,
}

pub fn config() -> MediaConfig {
    let mut config = MediaConfig::from_settings(&load_settings(&[]).expect("settings"));
    config.job_lease = LEASE;
    config.job_attempts = ATTEMPTS;
    config.job_retry_backoff = BACKOFF;
    config
}

pub fn hash(n: u32) -> FileHash {
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&n.to_be_bytes());
    FileHash::from_bytes(bytes)
}

pub fn micros(clock: &ManualClock) -> i64 {
    clock.now_utc().as_unix_micros()
}

impl Queue {
    pub async fn new() -> Queue {
        Queue::with_fixture(Fixture::new().await).await
    }

    pub async fn with_fixture(fixture: Fixture) -> Queue {
        let clock = Arc::new(ManualClock::new(START));
        let metrics = Arc::new(
            JobMetrics::register(
                &Metrics::new(BuildInfo {
                    version: "test",
                    commit: "test",
                    test_clock: true,
                })
                .expect("metrics"),
            )
            .expect("job metrics"),
        );
        let queue = Arc::new(JobQueue::new(
            fixture.api.clone(),
            clock.clone(),
            &config(),
            metrics.clone(),
        ));
        let account = Uuid::new_v4();
        let mut conn = fixture.owner_conn().await;
        let revision = account_with_revision(&mut conn, account, &[1; 32]).await;
        conn.close().await.expect("close owner connection");
        Queue {
            fixture,
            queue,
            clock,
            metrics,
            account,
            revision,
        }
    }

    /// Another account with a revision, for tests that cross accounts.
    pub async fn second_account(&self) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let mut conn = self.fixture.owner_conn().await;
        let revision = account_with_revision(&mut conn, account, &[2; 32]).await;
        conn.close().await.expect("close owner connection");
        (account, revision)
    }

    pub fn spec(&self, kind: JobKind, n: u32) -> JobSpec {
        self.spec_of(self.account, self.revision, kind, n)
    }

    pub fn spec_of(&self, account: Uuid, revision: Uuid, kind: JobKind, n: u32) -> JobSpec {
        JobSpec {
            account_id: account,
            revision_id: revision,
            kind,
            content_hash: hash(n),
            params_version: 1,
        }
    }

    pub async fn enqueue(&self, spec: JobSpec, priority: Priority) -> EnqueueOutcome {
        self.fixture
            .api
            .run(Scope::Account(spec.account_id), async |tx| {
                self.queue.enqueue_in_tx(tx, spec, priority).await
            })
            .await
            .expect("enqueue")
    }

    /// Enqueues a job of the default account and returns its id.
    pub async fn add(&self, kind: JobKind, n: u32, priority: Priority) -> Uuid {
        let spec = self.spec(kind, n);
        assert_eq!(self.enqueue(spec, priority).await, EnqueueOutcome::Enqueued);
        self.id_of(&spec).await
    }

    /// `count` jobs of one kind in one transaction, hashes `from..from + count`.
    pub async fn add_many(&self, kind: JobKind, from: u32, count: u32, priority: Priority) {
        self.fixture
            .api
            .run(Scope::Account(self.account), async |tx| {
                for n in from..from + count {
                    self.queue
                        .enqueue_in_tx(tx, self.spec(kind, n), priority)
                        .await?;
                }
                Ok::<_, DbError>(())
            })
            .await
            .expect("enqueue many");
    }

    pub async fn id_of(&self, spec: &JobSpec) -> Uuid {
        let mut conn = self.fixture.owner_conn().await;
        let id = sqlx::query_scalar(
            "SELECT id FROM jobs WHERE account_id = $1 AND content_hash = $2 AND params_version = $3 \
             AND kind = $4",
        )
        .bind(spec.account_id)
        .bind(spec.content_hash.as_bytes().as_slice())
        .bind(i16::try_from(spec.params_version).expect("params version"))
        .bind(match spec.kind {
            JobKind::Stills => "stills",
            JobKind::Poster => "poster",
            JobKind::Transcode => "transcode",
        })
        .fetch_one(&mut conn)
        .await
        .expect("job id");
        conn.close().await.expect("close owner connection");
        id
    }

    pub async fn row(&self, id: Uuid) -> Row {
        let mut conn = self.fixture.owner_conn().await;
        let row: Row = sqlx::query_as(
            "SELECT status, attempts::int, priority::int, leased_by, outcome, last_error, \
             (extract(epoch FROM available_at) * 1000000)::bigint AS available_at, \
             (extract(epoch FROM finished_at) * 1000000)::bigint AS finished_at FROM jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&mut conn)
        .await
        .expect("job row");
        conn.close().await.expect("close owner connection");
        let terminal = matches!(row.status.as_str(), "succeeded" | "dead");
        assert_eq!(
            row.finished_at.is_some(),
            terminal,
            "finished_at vs status: {row:?}"
        );
        assert!(
            row.outcome.is_none() || row.status == "succeeded",
            "outcome vs status: {row:?}"
        );
        row
    }

    pub async fn count(&self, sql: &'static str) -> i64 {
        let mut conn = self.fixture.owner_conn().await;
        let count = sqlx::query_scalar(sql)
            .fetch_one(&mut conn)
            .await
            .expect("count");
        conn.close().await.expect("close owner connection");
        count
    }

    pub fn clock_now(&self) -> sc_clock::Timestamp {
        self.clock.now_utc()
    }

    pub async fn finish(self) {
        let Queue { fixture, queue, .. } = self;
        drop(queue);
        fixture.finish().await;
    }
}
