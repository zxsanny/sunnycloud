//! ST-03-3 (AZ-3758 AC-3; NFT-SEC-18): range abuse on a 1 GiB file cannot amplify. Its own test binary with one
//! test, so no other test allocates while the memory is measured; heap bytes are counted at the allocator (as in
//! `progress_memory.rs`) and the resident set is read from `/proc/self/status`.
//!
//! At 03's boundary a request carries at most one range: 11 hands a multi-range or overlapping header (10,000
//! comma-separated ranges included) over as no range (`download_service.md`), so 03 answers one `Full` stream and
//! has no multipart form. The file is written through the real 03 upload path (`start_upload`, `put_block`,
//! `commit_upload`) on the real 02, 04, 05 and 07a.
//!
//! 04's reader verifies the first 64 KiB chunk at `open` and starts its read-ahead at one chunk, widening it only
//! while the consumer keeps pace (owner decision 2026-11-14), so an abusive stream that is not read holds about
//! one chunk: 1000 streams stay within 64 MiB.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use contracts::{
    ByteRange, ByteStream, CommitActor, CommitService, CommitUpload, Download, DownloadOptions,
    DownloadService, DownloadStatus, RevisionOrCurrent, StartUpload, StartUploadOutcome,
    StreamError, UploadService,
};
use futures_core::Stream;
use sc_audit::AuditWriter;
use sc_config::Secret;
use sc_ids::{BLOCK_SIZE, Hash32, hash_block};
use sc_testdb::TestDb;
use sunnycloud_blobstore::{
    BlobCatalog, BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, GcQueue,
    MasterKeyring, SyncBarrier,
};
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_transfer::{PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{PgTreeReader, PgTreeService, TreeConfig};
use uuid::Uuid;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to the system allocator; the counter only observes sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::SeqCst);
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const GIB: i64 = 1 << 30;
const STREAMS: usize = 1000;
const LIMIT: usize = 64 << 20;
/// 04's production default (`blob_read_ahead_chunks`).
const READ_AHEAD: u64 = 16;

/// One piece; the whole body of a block.
struct OnePiece(Option<Bytes>);

impl Stream for OnePiece {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.0.take().map(Ok))
    }
}

fn body(bytes: Vec<u8>) -> ByteStream {
    Box::pin(OnePiece(Some(Bytes::from(bytes))))
}

/// Block `index` of the file: deterministic bytes, made on demand so the file is never held whole.
fn block(index: u32, len: usize) -> Vec<u8> {
    let mut state = u64::from(index).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

fn rss_bytes() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("process status");
    let kib: usize = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .expect("VmRSS line");
    kib * 1024
}

async fn exec(db: &TestDb, sql: &str) {
    db.admin_exec(sql).await;
}

fn blob_config(volume: &std::path::Path, key_file: &std::path::Path) -> BlobstoreConfig {
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
        blob_read_ahead_chunks: READ_AHEAD,
        blob_io_threads: None,
        master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

fn transfer_config() -> TransferConfig {
    TransferConfig {
        upload_session_ttl: Duration::from_secs(24 * 3600),
        max_declared_size: 50 * (1 << 30),
        max_open_sessions: 8,
        max_staged: 1000,
        checkpoint_bytes: 8 << 20,
        checkpoint_interval: Duration::from_secs(1),
        max_lanes: 4,
        max_inflight_blocks_per_account: 32,
        batch_max_entries: 1000,
        commit_receipt_ttl: Duration::from_secs(24 * 3600),
        max_buffered_block_bytes: 1 << 30,
        session_reap_interval: Duration::from_secs(60),
        receipt_janitor_interval: Duration::from_secs(3600),
        max_vault_file_bytes: 104_857_600,
    }
}

/// The real 03 service over the real 02, 04, 05 and 07a on `db`, with its volume in `dir`.
async fn service(db: &TestDb, dir: &std::path::Path) -> PgUploadService {
    let api = db.api().await;
    let clock = sc_testdb::clock_at_start();
    let volume = dir.join("volume");
    std::fs::create_dir_all(&volume).expect("volume folder");
    let key_file = dir.join("master.key");
    std::fs::write(&key_file, [0x11u8; 32]).expect("key file");
    let config = blob_config(&volume, &key_file);
    let audit = AuditWriter::new(api.clone(), clock.clone());
    let keyring = Arc::new(MasterKeyring::new(
        config.clone(),
        api.clone(),
        audit.clone(),
    ));
    keyring.load_and_verify().await.expect("keyring starts");
    let reader = FileBlobReader::new(&config, keyring.clone(), api.clone(), audit, clock.clone())
        .expect("reader");
    let writer =
        FileBlobWriter::new(&config, keyring, SyncBarrier::new(Duration::ZERO)).expect("writer");
    let quota = QuotaConfig {
        default_quota_bytes: 10 * GIB,
    };
    let ledger = Arc::new(PgQuotaLedger::new(quota));
    let gc = GcQueue::new(&config, api.clone(), clock.clone()).expect("gc queue");
    let tree_config = TreeConfig {
        tombstone_horizon: Duration::from_secs(30 * 86_400),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size: 1000,
        purge_batch_size: 1000,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    };
    let commit = PgTreeService::new(
        api.clone(),
        Arc::new(StateGuard::default()),
        ledger.clone(),
        Arc::new(gc),
        Arc::new(PgFeedStamp),
        InProcessNotifier::new(),
        clock.clone(),
    );
    let ports = UploadPorts {
        guard: Arc::new(StateGuard::default()),
        tree: Arc::new(PgTreeReader::new(api.clone(), &tree_config)),
        commit: Arc::new(commit),
        reader: Arc::new(reader),
        commit_effects: Vec::new(),
        quota: ledger,
        quota_admin: Arc::new(PgQuotaAdmin::new(api.clone(), quota)),
        writer: Arc::new(writer),
        registry: Arc::new(BlobRegistry::new(&config, clock.clone()).expect("registry")),
        catalog: Arc::new(BlobCatalog::new(api.clone())),
    };
    PgUploadService::new(api, transfer_config(), ports, clock)
}

/// The account, its root, feed and quota rows; returns the root.
async fn account_rows(db: &TestDb, account: Uuid) -> Uuid {
    let root = Uuid::new_v4();
    exec(
        db,
        &format!(
            "INSERT INTO accounts (id, state, created_at, display_name, role) \
             VALUES ('{account}', 'active', 'epoch'::timestamptz, 'Fixture', 'user')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
             VALUES ('{root}', '{account}', NULL, 'folder', '', '', 1, 'epoch'::timestamptz, 'epoch'::timestamptz)"
        ),
    )
    .await;
    exec(
        db,
        &format!("INSERT INTO account_feeds (account_id, epoch) VALUES ('{account}', 1)"),
    )
    .await;
    exec(
        db,
        &format!("INSERT INTO quota (account_id) VALUES ('{account}')"),
    )
    .await;
    root
}

/// Uploads and commits a 1 GiB file; returns its node.
async fn one_gib_file(service: &PgUploadService, account: Uuid, root: Uuid) -> Uuid {
    let uploads: &dyn UploadService = service;
    let start = uploads
        .start_upload(
            account,
            StartUpload {
                parent_id: root,
                name: "big.bin".to_owned(),
                declared_size: GIB,
                client_key: "big".to_owned(),
                content_hash: None,
                base_revision: None,
                client_meta: None,
            },
        )
        .await
        .expect("start");
    let StartUploadOutcome::Started(started) = start else {
        panic!("expected a session, got {start:?}");
    };
    let blocks = u32::try_from(GIB as usize / BLOCK_SIZE).expect("block count");
    let mut blocklist: Vec<Hash32> = Vec::with_capacity(blocks as usize);
    for index in 0..blocks {
        let bytes = block(index, BLOCK_SIZE);
        let hash = hash_block(&bytes);
        blocklist.push(hash);
        uploads
            .put_block(
                account,
                started.upload_id,
                index,
                hash,
                body(bytes),
                BLOCK_SIZE as u64,
            )
            .await
            .expect("block");
    }
    let commits: &dyn CommitService = service;
    commits
        .commit_upload(
            CommitActor {
                account_id: account,
                device_label: "range".to_owned(),
            },
            CommitUpload {
                upload_id: started.upload_id,
                parent_id: root,
                name: "big.bin".to_owned(),
                base_revision: None,
                blocklist,
                client_mtime: None,
                client_meta: None,
                import_hint: false,
            },
        )
        .await
        .expect("commit")
        .node_id
}

/// Removes the temporary volume and key file on drop, also when an assertion fails.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("scratch {} not removed: {error}", self.0.display());
        }
    }
}

/// Waits until the read-ahead tasks stop allocating: the heap is unchanged across one 50 ms poll.
async fn settled() {
    let mut last = LIVE.load(Ordering::SeqCst);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let now = LIVE.load(Ordering::SeqCst);
        if now == last {
            return;
        }
        last = now;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn range_abuse_on_a_one_gib_file_answers_one_full_stream_and_stays_bounded() {
    // Arrange
    let db = TestDb::clone_template().await;
    let dir = std::env::temp_dir().join(format!("sc-range-{}", Uuid::new_v4()));
    let _scratch = Scratch(dir.clone());
    let account = Uuid::new_v4();
    let root = account_rows(&db, account).await;
    let service = service(&db, &dir).await;
    let node = one_gib_file(&service, account, root).await;
    let downloads: &dyn DownloadService = &service;
    let current = RevisionOrCurrent::Current(node);
    let open_end = DownloadOptions {
        range: Some(ByteRange {
            start: 0,
            end: u64::MAX,
        }),
        ..DownloadOptions::default()
    };
    let (heap_before, rss_before) = (LIVE.load(Ordering::SeqCst), rss_bytes());

    // Act
    let multi = downloads
        .open_revision_content(account, current, DownloadOptions::default())
        .await
        .expect("multi-range as no range");
    let mut streams: Vec<Download> = Vec::with_capacity(STREAMS);
    for _ in 0..STREAMS {
        streams.push(
            downloads
                .open_revision_content(account, current, open_end.clone())
                .await
                .expect("bytes=0-"),
        );
    }
    settled().await;
    let heap = LIVE.load(Ordering::SeqCst).saturating_sub(heap_before);
    let rss = rss_bytes().saturating_sub(rss_before);
    let statuses: Vec<DownloadStatus> = streams.iter().map(|download| download.status).collect();
    drop(streams);

    // Assert
    eprintln!(
        "{STREAMS} streams: heap growth {heap} B, RSS growth {rss} B, read-ahead {READ_AHEAD}"
    );
    assert_eq!((multi.status, multi.range), (DownloadStatus::Full, None));
    assert_eq!(multi.total_size, GIB);
    assert!(
        statuses
            .iter()
            .all(|status| matches!(status, DownloadStatus::Partial | DownloadStatus::Full)),
        "one stream per request"
    );
    assert!(heap <= LIMIT, "heap growth {heap} B above {LIMIT} B");
    assert!(rss <= LIMIT, "RSS growth {rss} B above {LIMIT} B");
    drop(multi);
    drop(service);
    assert_eq!(db.finish().await, 0, "connections left open");
}
