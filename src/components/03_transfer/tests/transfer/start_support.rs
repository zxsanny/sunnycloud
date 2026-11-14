//! `PgUploadService` on the real 02 reader and `publish`, 04 writer, reader, registry and GC queue (a temporary
//! volume), 05 feed stamp and 07a ledger, all on one test database. The account guard, the effects in 03's
//! composition-root slot and the account come from a `rig_wiring::Wiring`: by default the guard is the port backed
//! at its boundary and the slot is empty; `real_support` wires the real 01 guard and 08 media effect (AZ-3760). File
//! creation and the quota reservation can be made to fail through thin decorators that otherwise delegate to the
//! real writer and ledger; the reader counts its calls and `publish` can be paused (`commit_ports`). A restart is a new `PgUploadService` over the same ports (an empty progress table).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    AccountGuard, CommitEffect, StartUpload, StartUploadOutcome, Started, UploadError,
};
use sc_audit::AuditWriter;
use sc_clock::{Clock, ManualClock};
use sc_ids::FileHash;
use sc_names::{name_key, validate_name};
use sqlx::PgConnection;
use sunnycloud_blobstore::{
    BlobCatalog, BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, GcQueue,
    MasterKeyring, SyncBarrier,
};
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_transfer::{PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{PgTreeReader, PgTreeService};
use uuid::Uuid;

use crate::commit_ports::{CountingReader, PausingCommit};
use crate::rig_config::{blob_config, tree_config};
use crate::rig_wiring::{Site, Wiring, boundary};
use crate::start_ports::{FaultyLedger, FaultyWriter};
use crate::support::Fixture;

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

/// The ports and settings a service instance is built from; shared by every instance of a rig.
struct Parts {
    config: TransferConfig,
    guard: Arc<dyn AccountGuard>,
    effects: Vec<Arc<dyn CommitEffect>>,
    tree: Arc<PgTreeReader>,
    commit: Arc<PausingCommit>,
    reader: Arc<CountingReader>,
    quota: Arc<FaultyLedger>,
    quota_admin: Arc<PgQuotaAdmin>,
    registry: Arc<BlobRegistry>,
    catalog: Arc<BlobCatalog>,
}

pub struct Rig {
    pub fixture: Fixture,
    pub service: Arc<PgUploadService>,
    parts: Parts,
    pub writer: Arc<FaultyWriter>,
    pub ledger: Arc<FaultyLedger>,
    pub reader: Arc<CountingReader>,
    pub commit: Arc<PausingCommit>,
    pub clock: Arc<ManualClock>,
    pub account: Uuid,
    pub root: Uuid,
    pub volume: PathBuf,
    _scratch: Scratch,
}

impl Rig {
    /// An active account with its root and a quota of 10 GiB; at most 8 open sessions.
    pub async fn new() -> Rig {
        Rig::with_config(transfer_config()).await
    }

    /// As [`Rig::new`] with another transfer section.
    pub async fn with_config(config: TransferConfig) -> Rig {
        Rig::build(config, 0, false).await
    }

    /// As [`Rig::with_config`] with a free-space floor of `free_reserve` bytes and 04's preallocation as given.
    /// Used by the `volume_floor` test binary only.
    #[allow(dead_code)]
    pub async fn with_volume(config: TransferConfig, free_reserve: u64, preallocate: bool) -> Rig {
        Rig::build(config, free_reserve, preallocate).await
    }

    /// As [`Rig::new`] on a volume whose free-space floor is its whole size: the real 04 writer refuses every file.
    pub async fn below_floor() -> Rig {
        Rig::build(transfer_config(), u64::MAX, false).await
    }

    async fn build(config: TransferConfig, free_reserve: u64, preallocate: bool) -> Rig {
        Rig::build_on(Fixture::new().await, config, free_reserve, preallocate, 1).await
    }

    /// The rig over `fixture` (its component pool) with 04's read-ahead of `read_ahead` chunks.
    pub async fn build_on(
        fixture: Fixture,
        config: TransferConfig,
        free_reserve: u64,
        preallocate: bool,
        read_ahead: u64,
    ) -> Rig {
        let wire = async |site: Site| (boundary(site).await, ());
        let (rig, ()) =
            Rig::wired(fixture, config, free_reserve, preallocate, read_ahead, wire).await;
        rig
    }

    /// As [`Rig::build_on`] with the guard, slot effects and account `wire` makes once the volume exists; returns
    /// the rig and what `wire` kept for the test.
    pub async fn wired<T>(
        fixture: Fixture,
        config: TransferConfig,
        free_reserve: u64,
        preallocate: bool,
        read_ahead: u64,
        wire: impl AsyncFnOnce(Site) -> (Wiring, T),
    ) -> (Rig, T) {
        let clock = sc_testdb::clock_at_start();
        let dir = std::env::temp_dir().join(format!("sc-transfer-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let scratch = Scratch(dir.clone());
        let key_file = dir.join("master.key");
        fs::write(&key_file, [0x11u8; 32]).expect("key file");
        let blob_config = BlobstoreConfig {
            blob_preallocate: preallocate,
            blob_read_ahead_chunks: read_ahead,
            ..blob_config(&volume, &key_file, free_reserve)
        };
        let audit = AuditWriter::new(fixture.api.clone(), clock.clone());
        let keyring = Arc::new(MasterKeyring::new(
            blob_config.clone(),
            fixture.api.clone(),
            audit.clone(),
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        // After the keyring check, which wants an empty volume: a real `initialize` writes 05's epoch marker on it.
        let (wiring, kept) = wire(Site::new(&fixture, &blob_config, &clock).await).await;
        let reader = FileBlobReader::new(
            &blob_config,
            keyring.clone(),
            fixture.api.clone(),
            audit,
            clock.clone(),
        )
        .expect("reader");
        let reader = Arc::new(CountingReader::new(reader));
        let real = FileBlobWriter::new(&blob_config, keyring, SyncBarrier::new(Duration::ZERO))
            .expect("writer");
        let writer = Arc::new(FaultyWriter::new(real));
        let registry = BlobRegistry::new(&blob_config, clock.clone()).expect("registry");
        let quota = QuotaConfig {
            default_quota_bytes: 10 * GIB,
        };
        let ledger = Arc::new(FaultyLedger::new(PgQuotaLedger::new(quota)));
        let gc = GcQueue::new(&blob_config, fixture.api.clone(), clock.clone()).expect("gc queue");
        let commit = Arc::new(PausingCommit::new(PgTreeService::new(
            fixture.api.clone(),
            wiring.guard.clone(),
            ledger.clone(),
            Arc::new(gc),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            clock.clone(),
        )));
        let parts = Parts {
            config,
            guard: wiring.guard,
            effects: wiring.effects,
            tree: Arc::new(PgTreeReader::new(fixture.api.clone(), &tree_config())),
            commit: commit.clone(),
            reader: reader.clone(),
            quota: ledger.clone(),
            quota_admin: Arc::new(PgQuotaAdmin::new(fixture.api.clone(), quota)),
            registry: Arc::new(registry),
            catalog: Arc::new(BlobCatalog::new(fixture.api.clone())),
        };
        let service = instance(&fixture, &parts, &writer, &clock);
        let rig = Rig {
            fixture,
            service: Arc::new(service),
            parts,
            writer,
            ledger,
            reader,
            commit,
            clock,
            account: wiring.account,
            root: wiring.root,
            volume,
            _scratch: scratch,
        };
        (rig, kept)
    }

    /// A new instance over the same database, volume and ports: what a process restart leaves.
    pub fn instance(&self) -> PgUploadService {
        instance(&self.fixture, &self.parts, &self.writer, &self.clock)
    }

    /// Drops the current instance (its progress table) and serves from a new one.
    pub fn restart(&mut self) {
        self.service = Arc::new(self.instance());
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
            parts,
            writer,
            ledger,
            reader,
            commit,
            ..
        } = self;
        drop((service, parts, writer, ledger, reader, commit));
        fixture.finish().await;
    }
}

fn instance(
    fixture: &Fixture,
    parts: &Parts,
    writer: &Arc<FaultyWriter>,
    clock: &Arc<ManualClock>,
) -> PgUploadService {
    PgUploadService::new(
        fixture.api.clone(),
        parts.config.clone(),
        UploadPorts {
            guard: parts.guard.clone(),
            tree: parts.tree.clone(),
            commit: parts.commit.clone(),
            reader: parts.reader.clone(),
            commit_effects: parts.effects.clone(),
            quota: parts.quota.clone(),
            quota_admin: parts.quota_admin.clone(),
            writer: writer.clone(),
            registry: parts.registry.clone(),
            catalog: parts.catalog.clone(),
        },
        clock.clone(),
    )
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

/// The production defaults of the transfer section, with the 24 h TTL.
pub fn transfer_config() -> TransferConfig {
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
        max_vault_file_bytes: 104_857_600,
    }
}
