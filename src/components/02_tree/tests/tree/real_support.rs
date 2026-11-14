//! The tree with its real counterparts (AZ-3720, E-13/c16) on one test database and a scratch volume with a key
//! file: `PgTreeService` over 01's `IdentityAccounts` (guard and account admin), 07a's ledger, 04's GC queue and
//! 05's stamp and notifier; 03's `PgUploadService`, whose `commit_upload` and `commit_batch` hand `publish` the
//! real `BlobCommitEffect`, `ReceiptEffect` and `StagedCommitEffect`, with 08's `MediaEnqueueEffect` in the
//! composition root's effect slot; 08's `ThumbnailStates` and `DerivedStore` on the real 04 writer, reader,
//! registry and catalog. The account is created as 01 creates one: its row, then `initialize` of 07a, 05 and 02
//! in one transaction (01's sign-in that runs it needs a live OIDC provider). Probes run as the superuser.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    AccountAdmin, AccountLifecycleHook, BatchCommit, BatchEntry, BatchResult, CommitActor,
    CommitServiceError, CommitUpload, Committed, StartUpload, StartUploadOutcome, UploadError,
};
use media_api::{
    BlobPorts, DerivedStore, JobMetrics, JobPlanner, JobQueue, MediaConfig, MediaEnqueueEffect,
    ThumbnailStates,
};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_config::Secret;
use sc_db::Scope;
use sc_ids::{Hash32, hash_block};
use sc_observability::{BuildInfo, Metrics};
use sunnycloud_blobstore::{
    BlobCatalog, BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, FileEpochMarker,
    GcQueue, MasterKeyring, SyncBarrier,
};
use sunnycloud_identity::IdentityAccounts;
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedLifecycle, PgFeedStamp};
use sunnycloud_transfer::{PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{PgTreeLifecycle, PgTreeReader, PgTreeService};
use uuid::Uuid;

use crate::maintenance_support;
use crate::mutation_support::{DEFAULT_QUOTA, ProbeNotifier};
use crate::real_account_repo::create_account;
use crate::real_ports::{Faults, ObservedCommit, body};
use crate::support::Fixture;

pub const DEVICE: &str = "laptop";
const DAY: Duration = Duration::from_secs(86_400);

pub struct Real {
    pub fixture: Fixture,
    pub identity: Arc<IdentityAccounts>,
    pub service: Arc<PgTreeService>,
    pub commit: Arc<ObservedCommit>,
    pub upload: PgUploadService,
    pub faults: Arc<Faults>,
    pub queue: Arc<JobQueue>,
    pub planner: JobPlanner,
    pub store: DerivedStore,
    pub gc: Arc<GcQueue>,
    pub account: Uuid,
    pub root: Uuid,
    dir: PathBuf,
}

pub fn media_config() -> MediaConfig {
    MediaConfig {
        job_lease: Duration::from_secs(60),
        job_attempts: 3,
        job_retry_backoff: Duration::from_secs(1),
        bulk_queue_threshold: 200,
        job_terminal_ttl: DAY,
        job_reap_interval: Duration::from_secs(60),
        derived_janitor_interval: Duration::from_secs(3600),
        worker_status_interval: Duration::from_secs(30),
        derived_max_thumb: 1 << 20,
        derived_max_preview: 1 << 20,
        derived_max_transcode: 1 << 20,
        worker_api_token: Secret::new("worker-token".to_owned()),
        worker_api_token_next: None,
    }
}

fn transfer_config() -> TransferConfig {
    TransferConfig {
        upload_session_ttl: DAY,
        max_declared_size: 1 << 30,
        max_open_sessions: 8,
        max_staged: 1000,
        checkpoint_bytes: 8 << 20,
        checkpoint_interval: Duration::from_secs(1),
        max_lanes: 4,
        max_inflight_blocks_per_account: 32,
        batch_max_entries: 1000,
        commit_receipt_ttl: DAY,
        max_buffered_block_bytes: 1 << 30,
        session_reap_interval: Duration::from_secs(60),
        receipt_janitor_interval: Duration::from_secs(3600),
        max_vault_file_bytes: 104_857_600,
    }
}

impl Real {
    pub async fn new() -> Real {
        let fixture = Fixture::new().await;
        let clock = sc_testdb::clock_at_start();
        let dyn_clock: Arc<dyn Clock> = clock.clone();
        let api = fixture.api.clone();
        let dir = std::env::temp_dir().join(format!("sc-tree-real-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let key_file = dir.join("master.key");
        fs::write(&key_file, [0x11u8; 32]).expect("key file");
        let blobstore = BlobstoreConfig {
            master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
            ..crate::lifecycle_support::config(&volume)
        };
        let audit = AuditWriter::new(api.clone(), dyn_clock.clone());
        // The keyring starts on an empty volume, before any `blobs` row exists.
        let keyring = Arc::new(MasterKeyring::new(
            blobstore.clone(),
            api.clone(),
            audit.clone(),
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        let reader = Arc::new(
            FileBlobReader::new(
                &blobstore,
                keyring.clone(),
                api.clone(),
                audit,
                clock.clone(),
            )
            .expect("reader"),
        );
        let writer = Arc::new(
            FileBlobWriter::new(&blobstore, keyring, SyncBarrier::new(Duration::ZERO))
                .expect("writer"),
        );
        let registry =
            Arc::new(BlobRegistry::new(&blobstore, dyn_clock.clone()).expect("registry"));
        let catalog = Arc::new(BlobCatalog::new(api.clone()));
        let gc = Arc::new(GcQueue::new(&blobstore, api.clone(), dyn_clock.clone()).expect("gc"));
        let quota = QuotaConfig {
            default_quota_bytes: DEFAULT_QUOTA,
        };
        let quota_admin = Arc::new(PgQuotaAdmin::new(api.clone(), quota));
        let identity = Arc::new(IdentityAccounts::new(api.clone(), dyn_clock.clone()));
        let notifier = InProcessNotifier::new();
        let service = Arc::new(PgTreeService::new(
            api.clone(),
            identity.clone(),
            Arc::new(PgQuotaLedger::new(quota)),
            gc.clone(),
            Arc::new(PgFeedStamp),
            Arc::new(ProbeNotifier(notifier)),
            dyn_clock.clone(),
        ));
        let hooks: Vec<Arc<dyn AccountLifecycleHook>> = vec![
            quota_admin.clone(),
            Arc::new(PgFeedLifecycle::new(
                api.clone(),
                dyn_clock.clone(),
                Arc::new(FileEpochMarker::new(&blobstore).expect("epoch marker")),
            )),
            Arc::new(PgTreeLifecycle::new(
                api.clone(),
                gc.clone(),
                dyn_clock.clone(),
            )),
        ];
        let (account, root) = create_account(&fixture, &hooks).await;
        let media = media_config();
        let registry_metrics = Metrics::new(BuildInfo {
            version: "test",
            commit: "test",
            test_clock: true,
        })
        .expect("metrics");
        let metrics = Arc::new(JobMetrics::register(&registry_metrics).expect("job metrics"));
        let queue = Arc::new(JobQueue::new(
            api.clone(),
            dyn_clock.clone(),
            &media,
            metrics.clone(),
        ));
        let planner = JobPlanner::from_config(&media);
        let faults = Arc::new(Faults::default());
        let commit = Arc::new(ObservedCommit::new(service.clone()));
        let tree_reader = Arc::new(PgTreeReader::new(
            api.clone(),
            &maintenance_support::config(1000, 1000),
        ));
        let upload = PgUploadService::new(
            api.clone(),
            transfer_config(),
            UploadPorts {
                guard: identity.clone(),
                tree: tree_reader.clone(),
                commit: commit.clone(),
                reader: reader.clone(),
                commit_effects: vec![
                    Arc::new(MediaEnqueueEffect::new(queue.clone(), planner, account)),
                    faults.clone(),
                ],
                quota: Arc::new(PgQuotaLedger::new(quota)),
                quota_admin,
                writer: writer.clone(),
                registry: registry.clone(),
                catalog: catalog.clone(),
            },
            dyn_clock.clone(),
        );
        let store = DerivedStore::new(
            api.clone(),
            dyn_clock,
            tree_reader,
            BlobPorts {
                writer,
                reader,
                catalog,
                registry,
                gc: gc.clone(),
            },
            &media,
            metrics,
        );
        Real {
            fixture,
            identity,
            service,
            commit,
            upload,
            faults,
            queue,
            planner,
            store,
            gc,
            account,
            root,
            dir,
        }
    }

    pub fn actor(&self) -> CommitActor {
        CommitActor {
            account_id: self.account,
            device_label: DEVICE.to_owned(),
        }
    }

    /// The real `AccountAdmin::begin_deletion`, in the account's scope.
    pub async fn begin_deletion(&self) {
        self.fixture
            .api
            .run(Scope::Account(self.account), async |tx| {
                self.identity.begin_deletion(tx, self.account).await
            })
            .await
            .expect("begin deletion");
    }

    /// The real `ThumbnailStates` at the params version the planner enqueues with.
    pub fn thumbnail_states(&self) -> ThumbnailStates {
        ThumbnailStates::new(self.fixture.api.clone(), self.planner)
    }

    /// `data` staged under a new key through `stage_small`; returns the key.
    pub async fn stage(&self, data: &[u8]) -> Uuid {
        let key = Uuid::new_v4();
        self.upload
            .stage_small(
                self.account,
                key,
                Some(hash_block(data)),
                body(data),
                data.len() as u64,
            )
            .await
            .expect("staged");
        key
    }

    pub async fn commit_batch(
        &self,
        commit_id: Uuid,
        entries: Vec<BatchEntry>,
    ) -> Result<BatchResult, CommitServiceError> {
        self.upload
            .commit_batch(self.actor(), BatchCommit { commit_id, entries })
            .await
    }

    /// A large upload of `data` (at most one block) under the root: started, its block put; returns the
    /// session and the request that commits it as `name`.
    pub async fn upload_one(&self, name: &str, data: &[u8]) -> (Uuid, CommitUpload) {
        let started = self
            .upload
            .start_upload(
                self.account,
                StartUpload {
                    parent_id: self.root,
                    name: name.to_owned(),
                    declared_size: data.len() as i64,
                    client_key: Uuid::new_v4().to_string(),
                    content_hash: None,
                    base_revision: None,
                    client_meta: None,
                },
            )
            .await
            .expect("start upload");
        let StartUploadOutcome::Started(started) = started else {
            panic!("expected a started session, got {started:?}");
        };
        let block: Hash32 = hash_block(data);
        self.upload
            .put_block(
                self.account,
                started.upload_id,
                0,
                block,
                body(data),
                data.len() as u64,
            )
            .await
            .expect("put block");
        let request = CommitUpload {
            upload_id: started.upload_id,
            parent_id: self.root,
            name: name.to_owned(),
            base_revision: None,
            blocklist: vec![block],
            client_mtime: None,
            client_meta: None,
            import_hint: false,
        };
        (started.upload_id, request)
    }

    pub async fn commit_upload(
        &self,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        self.upload.commit_upload(self.actor(), request).await
    }

    pub async fn abort(&self, upload_id: Uuid) -> Result<(), UploadError> {
        self.upload.abort_upload(self.account, upload_id).await
    }

    pub async fn count(&self, sql: &str) -> i64 {
        self.fixture.db.admin_count(sql).await
    }

    /// Every row of the account a commit or a mutation can touch, as text.
    pub async fn rows(&self) -> String {
        let mut admin = self.fixture.db.admin().await;
        let text = sqlx::query_scalar(
            "SELECT concat_ws(' | ', \
             (SELECT string_agg(n::text, ',' ORDER BY n.id) FROM nodes n WHERE n.account_id = $1), \
             (SELECT string_agg(r::text, ',' ORDER BY r.id) FROM revisions r WHERE r.account_id = $1), \
             (SELECT string_agg(b::text, ',' ORDER BY b.id) FROM blobs b WHERE b.account_id = $1), \
             (SELECT string_agg(s::text, ',' ORDER BY s.id) FROM upload_sessions s WHERE s.account_id = $1), \
             (SELECT string_agg(g::text, ',' ORDER BY g.blob_id) FROM blob_gc g WHERE g.account_id = $1), \
             (SELECT string_agg(j::text, ',' ORDER BY j.id) FROM jobs j WHERE j.account_id = $1), \
             (SELECT string_agg(c::text, ',' ORDER BY c.commit_id) FROM commit_receipts c \
              WHERE c.account_id = $1), \
             (SELECT q::text FROM quota q WHERE q.account_id = $1), \
             (SELECT f::text FROM account_feeds f WHERE f.account_id = $1))",
        )
        .bind(self.account)
        .fetch_one(&mut admin)
        .await
        .expect("rows of the account");
        crate::mutation_support::close(admin).await;
        text
    }

    /// Job rows of the account for `hash`, by kind.
    pub async fn jobs_of(&self, hash: &sc_ids::FileHash) -> i64 {
        let hex: String = hash.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        self.count(&format!(
            "SELECT count(*) FROM jobs WHERE account_id = '{}' AND content_hash = '\\x{hex}'",
            self.account
        ))
        .await
    }

    pub async fn seq(&self) -> i64 {
        self.count(&format!(
            "SELECT seq FROM account_feeds WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    pub async fn finish(self) {
        let Real {
            fixture,
            dir,
            identity,
            service,
            commit,
            upload,
            faults,
            queue,
            store,
            gc,
            ..
        } = self;
        drop((identity, service, commit, upload, faults, queue, store, gc));
        if let Err(error) = fs::remove_dir_all(&dir) {
            eprintln!("scratch {} not removed: {error}", dir.display());
        }
        fixture.finish().await;
    }
}
