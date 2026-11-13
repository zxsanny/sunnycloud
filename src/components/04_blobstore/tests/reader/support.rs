use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use contracts::{
    BlobCatalog as _, BlobHandle, BlobRef, BlobRegistry as _, BlobWriter, ByteRange,
    PlaintextStream, RegisterState, StreamError, WrittenBlob, WrittenOrPlanned,
};
use sc_audit::AuditWriter;
use sc_config::Secret;
use sc_db::{Db, Scope};
use sc_testdb::{MIGRATOR, START, TestDb, clock_at_start};
use sunnycloud_blobstore::{
    BlobCatalog, BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, MasterKeyring,
    SyncBarrier, blob_path,
};
use uuid::Uuid;

pub const MIB: usize = 1 << 20;
pub const KEY: [u8; 32] = [0x22; 32];
const BLOCK: usize = 2 * MIB;

pub struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("scratch {} not removed: {error}", self.0.display());
        }
    }
}

pub struct Rig {
    pub db: TestDb,
    pub api: Db,
    pub _scratch: Scratch,
    pub volume: PathBuf,
    pub keyring: Arc<MasterKeyring>,
    pub writer: Arc<FileBlobWriter>,
    pub reader: FileBlobReader,
    pub registry: BlobRegistry,
    pub catalog: BlobCatalog,
    pub account: Uuid,
}

impl Rig {
    pub async fn new() -> Rig {
        Rig::with_window(16).await
    }

    pub async fn with_window(read_ahead: u64) -> Rig {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let dir = std::env::temp_dir().join(format!("sc-reader-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let scratch = Scratch(dir);
        let key_file = scratch.0.join("master.key");
        fs::write(&key_file, KEY).expect("key file");
        let config = config(&volume, &key_file, read_ahead);
        let clock = clock_at_start();
        let audit = AuditWriter::new(api.clone(), clock.clone());
        let keyring = Arc::new(MasterKeyring::new(
            config.clone(),
            api.clone(),
            audit.clone(),
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        let barrier = SyncBarrier::new(Duration::ZERO);
        let writer =
            Arc::new(FileBlobWriter::new(&config, Arc::clone(&keyring), barrier).expect("writer"));
        let reader = FileBlobReader::new(
            &config,
            Arc::clone(&keyring),
            api.clone(),
            audit,
            clock.clone(),
        )
        .expect("reader");
        let registry = BlobRegistry::new(&config, clock.clone()).expect("registry");
        let catalog = BlobCatalog::new(api.clone());
        let account = account(&db).await;
        Rig {
            db,
            api,
            _scratch: scratch,
            volume,
            keyring,
            writer,
            reader,
            registry,
            catalog,
            account,
        }
    }

    pub async fn finish(self) {
        let Rig {
            db,
            api,
            writer,
            keyring,
            reader,
            registry,
            catalog,
            ..
        } = self;
        drop((api, writer, keyring, reader, registry, catalog));
        assert_eq!(db.finish().await, 0, "connections left open");
    }

    pub fn path(&self, blob_id: Uuid) -> PathBuf {
        blob_path(&self.volume, blob_id)
    }

    /// Plans, creates and fills a blob of `data` in shuffled block order, registers it committed and reads its
    /// key material back through the catalog, as a caller of the reader does.
    pub async fn store(&self, data: &Arc<Vec<u8>>) -> BlobRef {
        let size = data.len();
        let plan = self.writer.plan(self.account, size as i64).expect("plan");
        let handle = self
            .writer
            .create_file(plan, size as i64, false)
            .await
            .expect("create_file");
        self.write_blocks(&handle, data, &shuffled(blocks_of(size), 0x5EED))
            .await;
        self.commit(handle, size as i64).await
    }

    pub async fn write_blocks(&self, handle: &BlobHandle, data: &[u8], order: &[u32]) {
        for &index in order {
            let token = self
                .writer
                .write_block(handle, index, block_bytes(data, index))
                .await
                .expect("block is written");
            self.writer
                .await_durable(token)
                .await
                .expect("block is durable");
        }
    }

    pub async fn commit(&self, handle: BlobHandle, size: i64) -> BlobRef {
        let blob_id = handle.plan.blob_id;
        self.api
            .run(Scope::Account(self.account), async |tx| {
                self.registry
                    .register_in_tx(
                        tx,
                        self.account,
                        WrittenOrPlanned::Written(WrittenBlob {
                            plan: handle.plan.clone(),
                            size,
                        }),
                        RegisterState::Committed,
                    )
                    .await
            })
            .await
            .expect("blob row");
        self.catalog
            .get_ref(self.account, blob_id)
            .await
            .expect("catalog lookup")
            .expect("committed blob has a reference")
    }

    pub async fn open(
        &self,
        blob: &BlobRef,
        range: Option<ByteRange>,
    ) -> Result<PlaintextStream, contracts::BlobError> {
        contracts::BlobReader::open(
            &self.reader,
            blob.blob_id,
            &blob.wrapped_key,
            blob.master_key_id,
            blob.size,
            range,
        )
        .await
    }

    pub async fn head(
        &self,
        blob: &BlobRef,
        max_bytes: u32,
    ) -> Result<Bytes, contracts::BlobError> {
        contracts::BlobReader::read_head(
            &self.reader,
            blob.blob_id,
            &blob.wrapped_key,
            blob.master_key_id,
            blob.size,
            max_bytes,
        )
        .await
    }

    pub async fn unavailable(&self, blob_id: Uuid) -> bool {
        self.db
            .admin_count(&format!(
                "SELECT count(*) FROM blobs WHERE id = '{blob_id}' AND unavailable_at IS NOT NULL"
            ))
            .await
            == 1
    }

    pub async fn findings(&self, blob_id: Uuid) -> i64 {
        self.db
            .admin_count(&format!(
                "SELECT count(*) FROM audit_log WHERE event = 'scrub_mismatch' AND subject_id = '{blob_id}'"
            ))
            .await
    }
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

pub fn config(volume: &Path, key_file: &Path, read_ahead: u64) -> BlobstoreConfig {
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
        scrub_max_rate: 1 << 20,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: false,
        blob_read_ahead_chunks: read_ahead,
        blob_io_threads: None,
        master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

/// Deterministic bytes that differ from block to block.
pub fn data(seed: u64, len: usize) -> Arc<Vec<u8>> {
    let mut state = seed | 1;
    Arc::new(
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 32) as u8
            })
            .collect(),
    )
}

pub fn blocks_of(size: usize) -> u32 {
    size.div_ceil(BLOCK).max(1) as u32
}

pub fn block_bytes(data: &[u8], index: u32) -> Bytes {
    let start = (index as usize * BLOCK).min(data.len());
    let end = (start + BLOCK).min(data.len());
    Bytes::copy_from_slice(&data[start..end])
}

pub fn shuffled(count: u32, seed: u64) -> Vec<u32> {
    let mut order: Vec<u32> = (0..count).collect();
    let mut state = seed | 1;
    for i in (1..order.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        order.swap(i, (state % (i as u64 + 1)) as usize);
    }
    order
}

pub fn range(start: usize, end: usize) -> Option<ByteRange> {
    Some(ByteRange {
        start: start as u64,
        end: end as u64,
    })
}

/// Everything a stream produced: the bytes of its `Ok` items, its first error, and the items after that error.
pub struct Consumed {
    pub bytes: Vec<u8>,
    pub error: Option<StreamError>,
    pub after_error: usize,
}

pub async fn consume(mut stream: PlaintextStream) -> Consumed {
    let mut consumed = Consumed {
        bytes: Vec::new(),
        error: None,
        after_error: 0,
    };
    while let Some(item) = next(&mut stream).await {
        match item {
            Ok(bytes) if consumed.error.is_none() => consumed.bytes.extend_from_slice(&bytes),
            Ok(_) => consumed.after_error += 1,
            Err(error) if consumed.error.is_none() => consumed.error = Some(error),
            Err(_) => consumed.after_error += 1,
        }
    }
    consumed
}

pub async fn next(stream: &mut PlaintextStream) -> Option<Result<Bytes, StreamError>> {
    std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
}
