//! `PgUploadService` on the real 02 reader, 04 writer and registry (a temporary volume) and 07a ledger, all on one
//! test database. The account guard is the port backed at its boundary (the real one is wired by E-14/c15). File
//! creation can be made to fail through a thin decorator that otherwise delegates to the real writer.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{StartUpload, StartUploadOutcome, Started, UploadError};
use sc_audit::AuditWriter;
use sc_clock::{Clock, ManualClock};
use sc_config::Secret;
use sc_ids::FileHash;
use sc_names::{name_key, validate_name};
use sqlx::PgConnection;
use sunnycloud_blobstore::{
    BlobRegistry, BlobstoreConfig, FileBlobWriter, MasterKeyring, SyncBarrier,
};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_transfer::{PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use uuid::Uuid;

use crate::start_ports::{FaultyWriter, StateGuard};
use crate::support::{Fixture, account};

pub const MIB: i64 = 1 << 20;
pub const GIB: i64 = 1 << 30;
pub const TTL: Duration = Duration::from_secs(24 * 3600);

/// Removes the temporary volume and key file on drop.
pub struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("scratch {} not removed: {error}", self.0.display());
        }
    }
}

pub struct Rig {
    pub fixture: Fixture,
    pub service: Arc<PgUploadService>,
    pub writer: Arc<FaultyWriter>,
    pub clock: Arc<ManualClock>,
    pub account: Uuid,
    pub root: Uuid,
    pub volume: PathBuf,
    _scratch: Scratch,
}

impl Rig {
    /// An active account with its root and a quota of 10 GiB; at most 8 open sessions.
    pub async fn new() -> Rig {
        let fixture = Fixture::new().await;
        let clock = sc_testdb::clock_at_start();
        let account = Uuid::new_v4();
        let mut owner = fixture.owner_conn().await;
        let root = account_rows(&mut owner, account).await;
        close(owner).await;
        let dir = std::env::temp_dir().join(format!("sc-transfer-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let scratch = Scratch(dir.clone());
        let key_file = dir.join("master.key");
        fs::write(&key_file, [0x11u8; 32]).expect("key file");
        let blob_config = blob_config(&volume, &key_file);
        let audit = AuditWriter::new(fixture.api.clone(), clock.clone());
        let keyring = Arc::new(MasterKeyring::new(
            blob_config.clone(),
            fixture.api.clone(),
            audit,
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        let real = FileBlobWriter::new(&blob_config, keyring, SyncBarrier::new(Duration::ZERO))
            .expect("writer");
        let writer = Arc::new(FaultyWriter::new(real));
        let registry = BlobRegistry::new(&blob_config, clock.clone()).expect("registry");
        let tree = PgTreeReader::new(fixture.api.clone(), &tree_config());
        let service = PgUploadService::new(
            fixture.api.clone(),
            transfer_config(),
            UploadPorts {
                guard: Arc::new(StateGuard),
                tree: Arc::new(tree),
                quota: Arc::new(PgQuotaLedger::new(QuotaConfig {
                    default_quota_bytes: 10 * GIB,
                })),
                writer: writer.clone(),
                registry: Arc::new(registry),
            },
            clock.clone(),
        );
        Rig {
            fixture,
            service: Arc::new(service),
            writer,
            clock,
            account,
            root,
            volume,
            _scratch: scratch,
        }
    }

    pub async fn start(&self, request: StartUpload) -> Result<StartUploadOutcome, UploadError> {
        self.service.start_upload(self.account, request).await
    }

    pub async fn reserved(&self) -> i64 {
        self.count(&format!(
            "SELECT reserved_bytes FROM quota WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    pub async fn sessions(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM upload_sessions WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    pub async fn uploading_blobs(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM blobs WHERE account_id = '{}' AND state = 'uploading'",
            self.account
        ))
        .await
    }

    /// Blob files on the volume.
    pub fn files(&self) -> usize {
        let mut found = 0;
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
                    found += 1;
                }
            }
        }
        found
    }

    pub async fn count(&self, sql: &str) -> i64 {
        self.fixture.db.admin_count(sql).await
    }

    pub async fn exec(&self, sql: &str) {
        self.fixture.db.admin_exec(sql).await;
    }

    pub async fn owner(&self) -> PgConnection {
        self.fixture.owner_conn().await
    }

    pub async fn finish(self) {
        let Rig {
            fixture,
            service,
            writer,
            ..
        } = self;
        drop((service, writer));
        fixture.finish().await;
    }
}

/// A start of `size` bytes named `name` under `parent` with the key `key`, without a hash.
pub fn request(parent: Uuid, name: &str, size: i64, key: &str) -> StartUpload {
    StartUpload {
        parent_id: parent,
        name: name.to_owned(),
        declared_size: size,
        client_key: key.to_owned(),
        content_hash: None,
        base_revision: None,
        client_meta: None,
    }
}

pub fn hash(byte: u8) -> FileHash {
    FileHash::from_bytes([byte; 32])
}

pub fn started(outcome: Result<StartUploadOutcome, UploadError>) -> Started {
    match outcome {
        Ok(StartUploadOutcome::Started(started)) => started,
        other => panic!("expected Started, got {other:?}"),
    }
}

pub async fn node(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let key = if name.is_empty() {
        String::new()
    } else {
        name_key(&validate_name(name).expect("valid name"))
            .as_str()
            .to_owned()
    };
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .bind(parent)
    .bind(kind)
    .bind(name)
    .bind(key)
    .execute(conn)
    .await
    .expect("insert node");
    id
}

/// A file under `parent` whose current revision (over a committed blob) has the hash `content`; returns the node
/// and the revision.
pub async fn file(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Uuid,
    name: &str,
    content: &FileHash,
) -> (Uuid, Uuid) {
    let file = node(conn, account, Some(parent), "file", name).await;
    let revision = revision(conn, account, file, content).await;
    sqlx::query("UPDATE nodes SET current_revision_id = $1 WHERE id = $2 AND account_id = $3")
        .bind(revision)
        .bind(file)
        .bind(account)
        .execute(conn)
        .await
        .expect("set current revision");
    (file, revision)
}

/// A revision of `node` over a fresh committed blob, not made current.
pub async fn revision(
    conn: &mut PgConnection,
    account: Uuid,
    node: Uuid,
    content: &FileHash,
) -> Uuid {
    let (blob, revision) = (Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
    )
    .bind(blob)
    .bind(account)
    .execute(&mut *conn)
    .await
    .expect("insert blob");
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
         VALUES ($1, $2, $3, $4, 1, $5, $6, 'epoch'::timestamptz)",
    )
    .bind(revision)
    .bind(account)
    .bind(node)
    .bind(blob)
    .bind(content.as_bytes().as_slice())
    .bind(vec![9u8; 32])
    .execute(conn)
    .await
    .expect("insert revision");
    revision
}

/// A committed session row of `key` (10 MiB, all 5 blocks received) whose commit created `revision`: no
/// reservation, expiring `TTL` after the test start (as AZ-3738's commit will write it).
pub async fn committed_session(conn: &mut PgConnection, account: Uuid, key: &str, revision: Uuid) {
    let blob: Uuid =
        sqlx::query_scalar("SELECT blob_id FROM revisions WHERE id = $1 AND account_id = $2")
            .bind(revision)
            .bind(account)
            .fetch_one(&mut *conn)
            .await
            .expect("revision blob");
    let expires = sc_testdb::START
        .checked_add(TTL)
        .expect("expiry")
        .as_unix_micros();
    sqlx::query(
        "INSERT INTO upload_sessions (id, account_id, blob_id, client_key, declared_size, block_count, \
         reserved_bytes, received, block_hashes, created_at, last_checkpoint_at, expires_at, committed_revision_id) \
         VALUES ($1, $2, $3, $4, $5, 5, 0, '\\x1f', $6, 'epoch'::timestamptz, 'epoch'::timestamptz, \
         'epoch'::timestamptz + $7::bigint * interval '1 microsecond', $8)",
    )
    .bind(Uuid::new_v4())
    .bind(account)
    .bind(blob)
    .bind(key)
    .bind(10 * MIB)
    .bind(vec![3u8; 5 * 32])
    .bind(expires)
    .bind(revision)
    .execute(conn)
    .await
    .expect("insert committed session");
}

pub async fn close(conn: PgConnection) {
    use sqlx::Connection;
    conn.close().await.expect("close fixture connection");
}

pub fn ticks_past_ttl(clock: &ManualClock) {
    clock.tick(TTL + Duration::from_secs(1));
}

/// Now on the rig's clock, for assertions on `expires_at`.
pub fn now(clock: &ManualClock) -> sc_clock::Timestamp {
    clock.now_utc()
}

/// The account, its root and its quota row (the installation default of 10 GiB applies); returns the root.
async fn account_rows(conn: &mut PgConnection, id: Uuid) -> Uuid {
    account(conn, id).await;
    let root = node(conn, id, None, "folder", "").await;
    sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
        .bind(id)
        .execute(conn)
        .await
        .expect("insert quota row");
    root
}

fn transfer_config() -> TransferConfig {
    TransferConfig {
        upload_session_ttl: TTL,
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
    }
}

fn tree_config() -> TreeConfig {
    TreeConfig {
        tombstone_horizon: Duration::from_secs(30 * 86_400),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size: 1000,
        purge_batch_size: 1000,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    }
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
        blob_read_ahead_chunks: 1,
        blob_io_threads: None,
        master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}
