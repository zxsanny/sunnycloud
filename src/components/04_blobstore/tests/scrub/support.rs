use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use contracts::{
    BlobCatalog as _, BlobHandle, BlobRef, BlobWriter, Due, GcQueue as _, GcReason, RegisterState,
    WrittenBlob, WrittenOrPlanned,
};
use sc_audit::AuditWriter;
use sc_clock::{CancellationToken, SystemClock};
use sc_config::Secret;
use sc_db::{Db, Scope};
use sc_observability::{BuildInfo, Metrics};
use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use sunnycloud_blobstore::{
    ActivityGauge, BlobCatalog, BlobRegistry, BlobScrubber, BlobstoreConfig, FileBlobReader,
    FileBlobWriter, GcQueue, GcSweeper, MasterKeyring, ScrubMetrics, SyncBarrier, blob_path,
};
use uuid::Uuid;

pub const MIB: usize = 1 << 20;
pub const KEY: [u8; 32] = [0x33; 32];
const BLOCK: usize = 2 * MIB;
/// Fast enough that a test never waits on the rate; the rate tests set their own.
pub const FAST: u64 = 4 << 30;

pub struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("scratch {} not removed: {error}", self.0.display());
        }
    }
}

pub struct Scrub {
    pub scrubber: Arc<BlobScrubber>,
    pub metrics: Metrics,
}

pub struct Rig {
    pub db: TestDb,
    pub api: Db,
    pub _scratch: Scratch,
    pub volume: PathBuf,
    pub clock: Arc<SystemClock>,
    pub audit: AuditWriter,
    pub keyring: Arc<MasterKeyring>,
    pub writer: Arc<FileBlobWriter>,
    pub reader: FileBlobReader,
    pub registry: Arc<BlobRegistry>,
    pub catalog: BlobCatalog,
    pub queue: GcQueue,
    pub gauge: ActivityGauge,
    pub config: BlobstoreConfig,
    pub account: Uuid,
}

impl Rig {
    pub async fn new() -> Rig {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let dir = std::env::temp_dir().join(format!("sc-scrub-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let scratch = Scratch(dir);
        let key_file = scratch.0.join("master.key");
        fs::write(&key_file, KEY).expect("key file");
        let config = config(&volume, &key_file);
        let clock = Arc::new(SystemClock::new());
        let audit = AuditWriter::new(api.clone(), clock.clone());
        let keyring = Arc::new(MasterKeyring::new(
            config.clone(),
            api.clone(),
            audit.clone(),
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        let gauge = ActivityGauge::new();
        let writer = Arc::new(
            FileBlobWriter::new(
                &config,
                Arc::clone(&keyring),
                SyncBarrier::new(Duration::ZERO),
            )
            .expect("writer")
            .with_activity(gauge.clone()),
        );
        let reader = FileBlobReader::new(
            &config,
            Arc::clone(&keyring),
            api.clone(),
            audit.clone(),
            clock.clone(),
        )
        .expect("reader")
        .with_activity(gauge.clone());
        let registry = Arc::new(BlobRegistry::new(&config, clock.clone()).expect("registry"));
        let catalog = BlobCatalog::new(api.clone());
        let queue = GcQueue::new(&config, api.clone(), clock.clone()).expect("queue");
        let account = account(&db).await;
        Rig {
            db,
            api,
            _scratch: scratch,
            volume,
            clock,
            audit,
            keyring,
            writer,
            reader,
            registry,
            catalog,
            queue,
            gauge,
            config,
            account,
        }
    }

    pub async fn finish(self) {
        let Rig {
            db,
            api,
            audit,
            keyring,
            writer,
            reader,
            registry,
            catalog,
            queue,
            ..
        } = self;
        drop((
            api, audit, keyring, writer, reader, registry, catalog, queue,
        ));
        assert_eq!(db.finish().await, 0, "connections left open");
    }

    /// A CLI-style scrubber: no gauge, the rig's configuration with the default rate of `FAST`.
    pub fn scrub(&self, batch: u32) -> Scrub {
        self.scrub_with(self.config.clone(), batch, None)
    }

    pub fn scrub_with(
        &self,
        config: BlobstoreConfig,
        batch: u32,
        gauge: Option<ActivityGauge>,
    ) -> Scrub {
        let metrics = Metrics::new(BuildInfo {
            version: "test",
            commit: "test",
            test_clock: false,
        })
        .expect("metrics");
        let series = ScrubMetrics::register(&metrics).expect("scrub series");
        let scrubber = BlobScrubber::new(
            &config,
            Arc::clone(&self.keyring),
            self.api.clone(),
            self.audit.clone(),
            self.clock.clone(),
            series,
            batch,
        )
        .expect("scrubber");
        let scrubber = match gauge {
            Some(gauge) => scrubber.with_activity(gauge),
            None => scrubber,
        };
        Scrub {
            scrubber: Arc::new(scrubber),
            metrics,
        }
    }

    pub fn sweeper(&self, hook: Option<Arc<dyn sunnycloud_blobstore::GcHook>>) -> GcSweeper {
        let sweeper = GcSweeper::new(
            &self.config,
            self.api.clone(),
            self.clock.clone(),
            Arc::clone(&self.registry),
            Vec::new(),
            CancellationToken::new(),
        )
        .expect("sweeper");
        match hook {
            Some(hook) => sweeper.with_hook(hook),
            None => sweeper,
        }
    }

    pub fn path(&self, blob_id: Uuid) -> PathBuf {
        blob_path(&self.volume, blob_id)
    }

    pub fn parked(&self, blob_id: Uuid) -> PathBuf {
        let mut name = self.path(blob_id).into_os_string();
        name.push(".gc");
        PathBuf::from(name)
    }

    pub async fn owner(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// A committed blob of `len` bytes written through the real writer, with its key material read back.
    pub async fn store(&self, len: usize, seed: u8) -> BlobRef {
        let plan = self.writer.plan(self.account, len as i64).expect("plan");
        let handle = self
            .writer
            .create_file(plan, false)
            .await
            .expect("create_file");
        let data = vec![seed; len];
        for index in 0..blocks_of(len) {
            self.write_block(&handle, &data, index).await;
        }
        let blob_id = handle.plan.blob_id;
        self.register(&handle, Some(len as i64), RegisterState::Committed)
            .await;
        self.catalog
            .get_ref(self.account, blob_id)
            .await
            .expect("catalog lookup")
            .expect("committed blob has a reference")
    }

    /// An `uploading` row with the first block of a two-block file written and the second one not.
    pub async fn half_uploaded(&self) -> Uuid {
        let len = BLOCK + MIB;
        let plan = self.writer.plan(self.account, len as i64).expect("plan");
        let handle = self
            .writer
            .create_file(plan, false)
            .await
            .expect("create_file");
        self.write_block(&handle, &vec![7; len], 0).await;
        self.register(&handle, None, RegisterState::Uploading).await;
        handle.plan.blob_id
    }

    async fn write_block(&self, handle: &BlobHandle, data: &[u8], index: u32) {
        let start = (index as usize * BLOCK).min(data.len());
        let end = (start + BLOCK).min(data.len());
        let token = self
            .writer
            .write_block(handle, index, Bytes::copy_from_slice(&data[start..end]))
            .await
            .expect("block is written");
        self.writer
            .await_durable(token)
            .await
            .expect("block is durable");
    }

    async fn register(&self, handle: &BlobHandle, size: Option<i64>, state: RegisterState) {
        let blob = match size {
            Some(size) => WrittenOrPlanned::Written(WrittenBlob {
                plan: handle.plan.clone(),
                size,
            }),
            None => WrittenOrPlanned::Planned(handle.plan.clone()),
        };
        self.api
            .run(Scope::Account(self.account), async |tx| {
                self.registry
                    .register_in_tx(tx, self.account, blob.clone(), state)
                    .await
            })
            .await
            .expect("blob row");
    }

    /// Queues the blob for removal now: its row becomes `gc_pending`.
    pub async fn schedule_removal(&self, blob_id: Uuid) {
        self.api
            .run(Scope::Account(self.account), async |tx| {
                self.queue
                    .schedule_in_tx(tx, blob_id, GcReason::Deleted, Due::Now)
                    .await
            })
            .await
            .expect("schedule removal");
    }

    pub fn flip_bit(&self, blob_id: Uuid) {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path(blob_id))
            .expect("blob file");
        let at = 64 + 20;
        let mut byte = [0];
        file.read_exact_at(&mut byte, at).expect("read byte");
        byte[0] ^= 0x01;
        file.write_all_at(&byte, at).expect("write byte");
    }

    pub fn delete_file(&self, blob_id: Uuid) {
        fs::remove_file(self.path(blob_id)).expect("remove blob file");
    }

    pub async fn count(&self, sql: &str) -> i64 {
        self.db.admin_count(sql).await
    }

    pub async fn unavailable(&self, blob_id: Uuid) -> bool {
        self.count(&format!(
            "SELECT count(*) FROM blobs WHERE id = '{blob_id}' AND unavailable_at IS NOT NULL"
        ))
        .await
            == 1
    }

    pub async fn scrubbed(&self, blob_id: Uuid) -> bool {
        self.count(&format!(
            "SELECT count(*) FROM blobs WHERE id = '{blob_id}' AND last_scrubbed_at IS NOT NULL"
        ))
        .await
            == 1
    }

    /// `last_scrubbed_at` of a row in microseconds since the epoch.
    pub async fn scrubbed_micros(&self, blob_id: Uuid) -> Option<i64> {
        let mut conn = self.owner().await;
        let micros = sqlx::query_scalar(
            "SELECT (EXTRACT(EPOCH FROM last_scrubbed_at) * 1000000)::bigint FROM blobs WHERE id = $1",
        )
        .bind(blob_id)
        .fetch_one(&mut conn)
        .await
        .expect("probe last_scrubbed_at");
        sqlx::Connection::close(conn).await.expect("close");
        micros
    }

    pub async fn events(&self, blob_id: Uuid) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM audit_log WHERE event = 'scrub_mismatch' AND subject_id = '{blob_id}'"
        ))
        .await
    }

    pub async fn all_events(&self) -> i64 {
        self.count("SELECT count(*) FROM audit_log WHERE event = 'scrub_mismatch'")
            .await
    }
}

fn blocks_of(len: usize) -> u32 {
    len.div_ceil(BLOCK).max(1) as u32
}

async fn account(db: &TestDb) -> Uuid {
    let id = Uuid::new_v4();
    let mut conn = db.conn_as(MIGRATOR).await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("insert account");
    sqlx::Connection::close(conn).await.expect("close");
    id
}

pub fn config(volume: &Path, key_file: &Path) -> BlobstoreConfig {
    BlobstoreConfig {
        blob_root: Some(volume.to_string_lossy().into_owned()),
        retention: Duration::from_secs(7 * 86_400),
        orphan_sweep_age: Duration::from_secs(86_400),
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: 0,
        blob_free_reserve_percent: 100,
        gc_interval: Duration::from_secs(60),
        gc_batch: 100,
        gc_claim_lease: Duration::from_secs(60),
        gc_cycle_budget: Duration::from_secs(60),
        scrub_interval: Duration::from_secs(3600),
        scrub_cycle_budget: Duration::from_secs(60),
        scrub_max_rate: FAST,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: false,
        blob_read_ahead_chunks: 4,
        blob_io_threads: None,
        master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

/// Runs the steps of a run until `done`; returns the totals and the number of steps taken.
pub async fn run_to_done(
    scrubber: &dyn contracts::Scrubber,
    run: &contracts::ScrubRun,
    max_steps: usize,
) -> (contracts::StepResult, usize) {
    let mut total = contracts::StepResult {
        checked: 0,
        mismatched: 0,
        missing: 0,
        done: false,
    };
    for steps in 1..=max_steps {
        let step = scrubber.run_step(run).await.expect("step");
        total.checked += step.checked;
        total.mismatched += step.mismatched;
        total.missing += step.missing;
        if step.done {
            total.done = true;
            return (total, steps);
        }
    }
    (total, max_steps)
}
