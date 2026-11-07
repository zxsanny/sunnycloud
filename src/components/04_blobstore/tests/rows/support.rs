use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobPlan, BlobRegistryError, GcQueueError};
use sc_clock::ManualClock;
use sc_config::Secret;
use sc_db::Db;
pub use sc_testdb::{MIGRATOR, START, TestDb, clock_at_start};
use sqlx::PgConnection;
use sunnycloud_blobstore::{BlobCatalog, BlobRegistry, BlobstoreConfig, GcQueue, blob_path};
use uuid::Uuid;

pub const DAY: Duration = Duration::from_secs(86_400);

pub const WRAPPED_KEY: &[u8] = b"wrapped-key-bytes";

/// Why a caller's unit of work stopped: a refused port call, or a deliberate rollback.
#[derive(Debug)]
pub enum Stop {
    Registry(BlobRegistryError),
    Queue(GcQueueError),
    Rollback,
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Registry(error) => write!(f, "registry refused: {error:?}"),
            Stop::Queue(error) => write!(f, "queue refused: {error:?}"),
            Stop::Rollback => f.write_str("rollback"),
        }
    }
}

impl From<BlobRegistryError> for Stop {
    fn from(error: BlobRegistryError) -> Self {
        Stop::Registry(error)
    }
}

impl From<GcQueueError> for Stop {
    fn from(error: GcQueueError) -> Self {
        Stop::Queue(error)
    }
}

pub struct Rig {
    pub db: TestDb,
    pub api: Db,
    pub root: PathBuf,
    pub clock: Arc<ManualClock>,
    pub registry: BlobRegistry,
    pub catalog: BlobCatalog,
    pub queue: GcQueue,
}

pub fn config(root: &std::path::Path, retention: Duration) -> BlobstoreConfig {
    BlobstoreConfig {
        blob_root: Some(root.to_string_lossy().into_owned()),
        retention,
        orphan_sweep_age: DAY,
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: 0,
        gc_interval: Duration::from_secs(60),
        gc_batch: 100,
        gc_claim_lease: Duration::from_secs(60),
        gc_cycle_budget: Duration::from_secs(60),
        scrub_interval: Duration::from_secs(3600),
        scrub_cycle_budget: Duration::from_secs(60),
        scrub_max_rate: 1 << 20,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: false,
        blob_read_ahead_chunks: 1,
        blob_io_threads: None,
        master_key_file: Secret::new("unused".to_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

impl Rig {
    pub async fn new() -> Rig {
        Rig::with_retention(7 * DAY).await
    }

    pub async fn with_retention(retention: Duration) -> Rig {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let root = std::env::temp_dir().join(format!("sc-rows-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("blob root");
        let config = config(&root, retention);
        let clock = clock_at_start();
        let registry = BlobRegistry::new(&config, clock.clone()).expect("registry");
        let catalog = BlobCatalog::new(api.clone());
        let queue = GcQueue::new(&config, api.clone(), clock.clone()).expect("queue");
        Rig {
            db,
            api,
            root,
            clock,
            registry,
            catalog,
            queue,
        }
    }

    pub async fn finish(self) {
        let Rig {
            db,
            api,
            root,
            registry,
            catalog,
            queue,
            ..
        } = self;
        drop((api, registry, catalog, queue));
        if let Err(error) = fs::remove_dir_all(&root) {
            eprintln!("scratch {} not removed: {error}", root.display());
        }
        assert_eq!(db.finish().await, 0, "connections left open");
    }

    pub async fn owner(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// A real, empty file where the store keeps the blob: `reinstate_in_tx` only looks for its presence.
    pub fn put_file(&self, blob_id: Uuid) {
        let path = blob_path(&self.root, blob_id);
        fs::create_dir_all(path.parent().expect("blob folder")).expect("folders");
        fs::write(path, b"x").expect("blob file");
    }

    pub async fn row(&self, blob_id: Uuid) -> Option<(String, Option<i64>)> {
        let mut conn = self.owner().await;
        let row = sqlx::query_as("SELECT state, size FROM blobs WHERE id = $1")
            .bind(blob_id)
            .fetch_optional(&mut conn)
            .await
            .expect("probe blobs");
        conn_close(conn).await;
        row
    }

    /// Due time in microseconds since the epoch and the reason of the queue row.
    pub async fn queue_row(&self, blob_id: Uuid) -> Option<(i64, String)> {
        let mut conn = self.owner().await;
        let row = sqlx::query_as(
            "SELECT (EXTRACT(EPOCH FROM due_at) * 1000000)::bigint, reason \
             FROM blob_gc WHERE blob_id = $1",
        )
        .bind(blob_id)
        .fetch_optional(&mut conn)
        .await
        .expect("probe blob_gc");
        conn_close(conn).await;
        row
    }

    pub async fn created_micros(&self, blob_id: Uuid) -> i64 {
        let mut conn = self.owner().await;
        let micros = sqlx::query_scalar(
            "SELECT (EXTRACT(EPOCH FROM created_at) * 1000000)::bigint FROM blobs WHERE id = $1",
        )
        .bind(blob_id)
        .fetch_one(&mut conn)
        .await
        .expect("probe created_at");
        conn_close(conn).await;
        micros
    }

    pub async fn queue_rows(&self) -> i64 {
        let mut conn = self.owner().await;
        let count = sqlx::query_scalar("SELECT count(*) FROM blob_gc")
            .fetch_one(&mut conn)
            .await
            .expect("count blob_gc");
        conn_close(conn).await;
        count
    }
}

async fn conn_close(conn: PgConnection) {
    sqlx::Connection::close(conn).await.expect("close probe");
}

pub fn plan(blob_id: Uuid) -> BlobPlan {
    BlobPlan {
        blob_id,
        wrapped_key: WRAPPED_KEY.to_vec(),
        master_key_id: 3,
    }
}

pub async fn account(rig: &Rig) -> Uuid {
    let id = Uuid::new_v4();
    let mut conn = rig.owner().await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("insert account");
    conn_close(conn).await;
    id
}

/// A committed row written as the schema owner, so a test starts from a known state.
pub async fn committed_blob(rig: &Rig, account: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    let mut conn = rig.owner().await;
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', 10, $3, 3, 'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
    )
    .bind(id)
    .bind(account)
    .bind(WRAPPED_KEY)
    .bind(START.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("insert blob");
    conn_close(conn).await;
    id
}
