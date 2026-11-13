//! The output acceptance (AZ-3842) over the real 04 writer, reader, registry, GC queue and sweeper and the real 02
//! `PgTreeReader`, on the queue harness: jobs are enqueued and claimed through the real queue, and the tree reader
//! or the registry can be wrapped to land a revision collection or a fault at a chosen step.

use std::sync::Arc;

use bytes::Bytes;
use contracts::{AcceptOutcome, ByteStream, DerivedKind, OutputMeta, TreeReader, WorkerJobError};
use media_api::{BlobPorts, DerivedStore, MediaConfig, Priority};
use media_shared::types::JobKind;
use sc_clock::CancellationToken;
use sqlx::Connection;
use sunnycloud_blobstore::{BlobCatalog, GcQueue, GcSweeper};
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use uuid::Uuid;

use crate::blob_support::{Store, WEBP};
use crate::queue_support::{Queue, config, hash};
use crate::support::{Fixture, load_settings};

pub const WORKER: &str = "w1";

/// The head of an MP4 with the index first (`ftyp` isom), followed by the given payload.
pub fn mp4(payload_len: usize) -> Vec<u8> {
    let mut bytes = b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41".to_vec();
    bytes.resize(bytes.len() + payload_len, 0x5a);
    bytes
}

pub fn webp() -> Vec<u8> {
    WEBP.to_vec()
}

/// A 1x1 PNG head.
pub fn png() -> Vec<u8> {
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00"
        .to_vec()
}

pub fn meta(media_type: &str, width: Option<u32>, height: Option<u32>) -> OutputMeta {
    OutputMeta {
        media_type: media_type.to_owned(),
        width,
        height,
        duration_ms: None,
    }
}

pub fn body(bytes: Vec<u8>) -> ByteStream {
    Box::pin(futures_util::stream::iter([Ok(Bytes::from(bytes))]))
}

pub struct Accept {
    pub q: Queue,
    pub blobs: Store,
    pub tree: Arc<PgTreeReader>,
    pub gc: Arc<GcQueue>,
    pub store: Arc<DerivedStore>,
}

/// The derived row of a key as the schema owner reads it.
#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Derived {
    pub kind: String,
    pub blob_id: Uuid,
    pub media_type: String,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

impl Accept {
    pub async fn new() -> Accept {
        Accept::with(config(), |tree| tree, |store| store.registry.clone()).await
    }

    /// `media` is the 08 configuration; `tree` and `registry` may wrap the real ports.
    pub async fn with(
        media: MediaConfig,
        tree: impl FnOnce(Arc<dyn TreeReader>) -> Arc<dyn TreeReader>,
        registry: impl FnOnce(&Store) -> Arc<dyn contracts::BlobRegistry>,
    ) -> Accept {
        // The keyring starts on an empty volume, before any `blobs` row exists.
        let fixture = Fixture::new().await;
        let blobs = Store::new(&fixture).await;
        let q = Queue::with_fixture(fixture).await;
        let settings = load_settings(&[]).expect("settings");
        let real_tree = Arc::new(PgTreeReader::new(
            q.fixture.api.clone(),
            &TreeConfig::from_settings(&settings),
        ));
        let gc = Arc::new(
            GcQueue::new(&blobs.config, q.fixture.api.clone(), q.clock.clone()).expect("gc queue"),
        );
        let ports = BlobPorts {
            writer: blobs.writer.clone(),
            reader: blobs.reader.clone(),
            catalog: Arc::new(BlobCatalog::new(q.fixture.api.clone())),
            registry: registry(&blobs),
            gc: gc.clone(),
        };
        let store = Arc::new(DerivedStore::new(
            q.fixture.api.clone(),
            q.clock.clone(),
            tree(real_tree.clone()),
            ports,
            &media,
        ));
        Accept {
            q,
            blobs,
            tree: real_tree,
            gc,
            store,
        }
    }

    /// Enqueues a job of `kind` over content `n` and claims it for `WORKER`; returns its id.
    pub async fn claimed(&self, kind: JobKind, n: u32) -> Uuid {
        let id = self.q.add(kind, n, Priority::Interactive).await;
        let claimed = self
            .q
            .queue
            .claim(kind.lane(), WORKER)
            .await
            .expect("claim")
            .expect("a due job");
        assert_eq!(claimed.job_id, id, "the job just enqueued is claimed");
        id
    }

    pub async fn post(
        &self,
        job: Uuid,
        kind: DerivedKind,
        meta: OutputMeta,
        bytes: Vec<u8>,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        let length = u64::try_from(bytes.len()).expect("length");
        self.store
            .accept_output(job, WORKER, kind, meta, body(bytes), length)
            .await
    }

    pub async fn post_webp(&self, job: Uuid, kind: DerivedKind) -> AcceptOutcome {
        self.post(job, kind, meta("image/webp", Some(1), Some(1)), webp())
            .await
            .expect("a WebP output is accepted")
    }

    pub async fn derived(&self, n: u32) -> Vec<Derived> {
        let mut conn = self.q.fixture.owner_conn().await;
        let rows = sqlx::query_as(
            "SELECT kind, blob_id, media_type, size, width, height FROM derived \
             WHERE account_id = $1 AND content_hash = $2 AND params_version = 1 ORDER BY kind",
        )
        .bind(self.q.account)
        .bind(hash(n).as_bytes().as_slice())
        .fetch_all(&mut conn)
        .await
        .expect("derived rows");
        conn.close().await.expect("close owner connection");
        rows
    }

    /// `(state, gc reason, due now)` of each of the account's blob rows but the fixture revision's.
    pub async fn blob_rows(&self) -> Vec<(Uuid, String, Option<String>, Option<bool>)> {
        let mut conn = self.q.fixture.owner_conn().await;
        let rows = sqlx::query_as(
            "SELECT b.id, b.state, g.reason, g.due_at = ('epoch'::timestamptz + $2::bigint * interval '1 microsecond') \
             FROM blobs b LEFT JOIN blob_gc g ON g.blob_id = b.id \
             WHERE b.account_id = $1 AND NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = b.id) ORDER BY b.id",
        )
        .bind(self.q.account)
        .bind(self.q.clock_now().as_unix_micros())
        .fetch_all(&mut conn)
        .await
        .expect("blob rows");
        conn.close().await.expect("close owner connection");
        rows
    }

    /// One 04 GC cycle at the queue clock's now.
    pub async fn sweep(&self) -> u64 {
        let sweeper = GcSweeper::new(
            &self.blobs.config,
            self.q.fixture.api.clone(),
            self.q.clock.clone(),
            self.blobs.registry.clone(),
            Vec::new(),
            CancellationToken::new(),
        )
        .expect("sweeper");
        sweeper
            .run_cycle(self.q.clock_now())
            .await
            .expect("gc cycle")
            .removed
    }

    pub async fn quota_used(&self) -> i64 {
        let mut conn = self.q.fixture.owner_conn().await;
        let used = sqlx::query_scalar(
            "SELECT coalesce(sum(used_bytes), 0)::bigint FROM quota WHERE account_id = $1",
        )
        .bind(self.q.account)
        .fetch_one(&mut conn)
        .await
        .expect("quota");
        conn.close().await.expect("close owner connection");
        used
    }

    pub async fn finish(self) {
        let Accept {
            q,
            blobs,
            tree,
            gc,
            store,
        } = self;
        drop((store, gc, tree, blobs));
        q.finish().await;
    }
}
