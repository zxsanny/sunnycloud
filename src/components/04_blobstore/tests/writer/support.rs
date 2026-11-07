use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use contracts::{BlobHandle, BlobPlan, BlobWriter, ByteStream, StreamError};
use futures_core::Stream;
use sc_audit::AuditWriter;
use sc_config::Secret;
use sc_db::Db;
use sc_testdb::{TestDb, clock_at_start};
use sunnycloud_blobstore::{
    BlobLayout, BlobstoreConfig, ChunkCipher, FileBlobWriter, HEADER_LEN, Header, MasterKeyring,
    SyncBarrier, SyncHook, blob_path,
};
use uuid::Uuid;

pub const MIB: usize = 1 << 20;
pub const KEY: [u8; 32] = [0x11; 32];

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
    pub scratch: Scratch,
    pub volume: PathBuf,
    pub keyring: Arc<MasterKeyring>,
    pub hook: Arc<SyncHook>,
    pub writer: Arc<FileBlobWriter>,
}

pub struct Settings {
    pub reserve: u64,
    pub preallocate: bool,
    pub io_threads: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            reserve: 0,
            preallocate: true,
            io_threads: None,
        }
    }
}

impl Rig {
    pub async fn new() -> Rig {
        Rig::with(Settings::default()).await
    }

    pub async fn with(settings: Settings) -> Rig {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let dir = std::env::temp_dir().join(format!("sc-writer-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let scratch = Scratch(dir);
        let key_file = scratch.0.join("master.key");
        fs::write(&key_file, KEY).expect("key file");
        let config = config(&volume, &key_file, &settings);
        let audit = AuditWriter::new(api.clone(), clock_at_start());
        let keyring = Arc::new(MasterKeyring::new(config.clone(), api.clone(), audit));
        keyring.load_and_verify().await.expect("keyring starts");
        let hook = SyncHook::new();
        let barrier = SyncBarrier::with_hook(Duration::ZERO, Arc::clone(&hook));
        let writer =
            Arc::new(FileBlobWriter::new(&config, Arc::clone(&keyring), barrier).expect("writer"));
        Rig {
            db,
            api,
            scratch,
            volume,
            keyring,
            hook,
            writer,
        }
    }

    pub async fn finish(self) {
        let Rig {
            db,
            api,
            writer,
            keyring,
            ..
        } = self;
        drop((api, writer, keyring));
        assert_eq!(db.finish().await, 0, "connections left open");
    }

    /// A writer on the same volume whose keyring never loaded its keys.
    pub fn unloaded_writer(&self) -> FileBlobWriter {
        let settings = Settings::default();
        let config = config(&self.volume, &self.scratch.0.join("master.key"), &settings);
        let audit = AuditWriter::new(self.api.clone(), clock_at_start());
        let keyring = Arc::new(MasterKeyring::new(config.clone(), self.api.clone(), audit));
        FileBlobWriter::new(&config, keyring, SyncBarrier::new(Duration::ZERO)).expect("writer")
    }

    pub fn path(&self, blob_id: Uuid) -> PathBuf {
        blob_path(&self.volume, blob_id)
    }

    pub fn blob_files(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![self.volume.join("blobs")];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.expect("directory entry").path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    found.push(path);
                }
            }
        }
        found
    }

    pub async fn rows_for(&self, blob_id: Uuid) -> i64 {
        self.db
            .admin_count(&format!(
                "SELECT count(*) FROM blobs WHERE id = '{blob_id}'"
            ))
            .await
    }
}

fn config(volume: &Path, key_file: &Path, settings: &Settings) -> BlobstoreConfig {
    BlobstoreConfig {
        blob_root: Some(volume.to_string_lossy().into_owned()),
        retention: Duration::from_secs(7 * 86_400),
        orphan_sweep_age: Duration::from_secs(86_400),
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: settings.reserve,
        gc_interval: Duration::from_secs(60),
        gc_batch: 100,
        gc_claim_lease: Duration::from_secs(60),
        gc_cycle_budget: Duration::from_secs(60),
        scrub_interval: Duration::from_secs(3600),
        scrub_cycle_budget: Duration::from_secs(60),
        scrub_max_rate: 1 << 20,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: settings.preallocate,
        blob_read_ahead_chunks: 1,
        blob_io_threads: settings.io_threads,
        master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

pub fn marker(seed: u64) -> [u8; 64] {
    let mut bytes = [b'-'; 64];
    let text = format!("SUNNYCLOUD-MARKER-{seed:016x}");
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    bytes
}

/// Deterministic bytes with the marker at every 1 MiB.
pub fn data(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed | 1;
    let mut bytes: Vec<u8> = (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 32) as u8
        })
        .collect();
    let marker = marker(seed);
    for start in (0..len).step_by(MIB) {
        if start + marker.len() <= len {
            bytes[start..start + marker.len()].copy_from_slice(&marker);
        }
    }
    bytes
}

pub fn seed_of(size: usize) -> u64 {
    size as u64 + 7
}

pub fn blocks_of(size: usize) -> u32 {
    size.div_ceil(2 * MIB).max(1) as u32
}

pub fn block_bytes(data: &[u8], index: u32) -> Bytes {
    let start = (index as usize * 2 * MIB).min(data.len());
    let end = (start + 2 * MIB).min(data.len());
    Bytes::copy_from_slice(&data[start..end])
}

pub async fn write_in_order(writer: &FileBlobWriter, handle: &BlobHandle, data: &[u8]) {
    for index in 0..blocks_of(data.len()) {
        let token = writer
            .write_block(handle, index, block_bytes(data, index))
            .await
            .expect("block is written");
        writer.await_durable(token).await.expect("block is durable");
    }
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

/// Blocks in shuffled order from 4 tasks, each awaited durable.
pub async fn write_shuffled(
    writer: &Arc<FileBlobWriter>,
    handle: &BlobHandle,
    data: &Arc<Vec<u8>>,
) {
    let order = shuffled(blocks_of(data.len()), 0x5EED);
    let mut tasks = Vec::new();
    for lane in 0..4 {
        let share: Vec<u32> = order.iter().copied().skip(lane).step_by(4).collect();
        let (writer, handle, data) = (Arc::clone(writer), handle.clone(), Arc::clone(data));
        tasks.push(tokio::spawn(async move {
            for index in share {
                let token = writer
                    .write_block(&handle, index, block_bytes(&data, index))
                    .await
                    .expect("block is written");
                writer.await_durable(token).await.expect("block is durable");
            }
        }));
    }
    for task in tasks {
        task.await.expect("lane finishes");
    }
}

pub async fn created(rig: &Rig, size: usize, preallocate: bool) -> (BlobHandle, Arc<Vec<u8>>) {
    let plan = rig.writer.plan(Uuid::new_v4(), size as i64).expect("plan");
    let handle = rig
        .writer
        .create_file(plan, preallocate)
        .await
        .expect("create_file");
    (handle, Arc::new(data(seed_of(size), size)))
}

/// Decrypts the whole file with the format's own cipher; the length is checked against the layout first.
pub fn decrypt(rig: &Rig, plan: &BlobPlan, size: usize) -> Vec<u8> {
    let bytes = fs::read(rig.path(plan.blob_id)).expect("blob file");
    let layout = BlobLayout::new(size as u64).expect("layout");
    assert_eq!(bytes.len() as u64, layout.file_len(), "file length");
    let header = Header::decode(&bytes[..HEADER_LEN]).expect("header");
    let key = rig
        .keyring
        .unwrap_data_key(plan.blob_id, &plan.wrapped_key, plan.master_key_id)
        .expect("data key");
    let cipher = ChunkCipher::new(&key, header.nonce_prefix, plan.blob_id);
    let mut plain = Vec::new();
    for index in 0..layout.chunk_count() {
        let start = layout.chunk_offset(index).expect("offset") as usize;
        let len = layout.plain_len(index).expect("length") as usize + 16;
        let mut chunk = bytes[start..start + len].to_vec();
        cipher
            .open_chunk(&layout, index, &mut chunk)
            .expect("chunk verifies");
        plain.extend_from_slice(&chunk);
    }
    plain
}

struct Items(VecDeque<Result<Bytes, StreamError>>);

impl Stream for Items {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop_front())
    }
}

/// The bytes as a stream of pieces of `piece` bytes, so that block and chunk edges fall inside pieces.
pub fn stream_of(bytes: &[u8], piece: usize) -> ByteStream {
    let items = bytes
        .chunks(piece)
        .map(|part| Ok(Bytes::copy_from_slice(part)))
        .collect();
    Box::pin(Items(items))
}

/// Delivers `bytes`, then fails.
pub fn failing_after(bytes: &[u8]) -> ByteStream {
    let mut items: VecDeque<_> = bytes
        .chunks(100_000)
        .map(|part| Ok(Bytes::copy_from_slice(part)))
        .collect();
    items.push_back(Err(StreamError::Source(Box::new(std::io::Error::other(
        "source failed",
    )))));
    Box::pin(Items(items))
}
