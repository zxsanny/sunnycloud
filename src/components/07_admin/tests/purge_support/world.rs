//! The purger over the real components on a real PostgreSQL 18 and a scratch blob volume (IT-07-9 to IT-07-13):
//! 01's `IdentityAccounts` (claim and finalize), the erase hooks of 06 (`PgShareLifecycle`), 02
//! (`PgTreeLifecycle`), 03 (`PgTransferLifecycle` over a real `PgUploadService`), 08 (`MediaLifecycle`), 05
//! (`PgFeedLifecycle`) and 07a (`PgQuotaAdmin`) in the v1.0 production order, 04's GC queue and sweeper (with 02's
//! and 08's owner cleanups), and 07's deletion request. Components connect as the API role; fixtures and probes
//! use the schema owner.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{AccountAdmin, AccountDeletionService, AccountLifecycleHook, BlobOwnerCleanup};
use media_api::MediaLifecycle;
use sc_audit::AuditWriter;
use sc_clock::{CancellationToken, Clock};
use sc_config::{ApiSettings, Environment, Secret};
use sc_db::Db;
use sc_testdb::{MIGRATOR, TestDb};
use sqlx::PgConnection;
use sunnycloud_admin::{AccountDeletionServiceImpl, AdminConfig, DeletionPurger};
use sunnycloud_blobstore::{
    BlobCatalog, BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, FileEpochMarker,
    GcQueue, GcSweeper, MasterKeyring, SyncBarrier,
};
use sunnycloud_identity::IdentityAccounts;
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sharing::PgShareLifecycle;
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedLifecycle, PgFeedStamp};
use sunnycloud_transfer::{PgTransferLifecycle, PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{PgTreeLifecycle, PgTreeReader, PgTreeService, TreeConfig};
use uuid::Uuid;

use crate::support::settings::load_settings;

/// The harness values of the purger keys (07 `tests.md`): batch 50, cycle budget 300 ms, alert after 20 s.
pub const PURGER_KEYS: [(&str, &str); 3] = [
    ("SUNNYCLOUD_ACCOUNT_DELETE_BATCH_SIZE", "50"),
    ("SUNNYCLOUD_ACCOUNT_DELETE_CYCLE_BUDGET", "300ms"),
    ("SUNNYCLOUD_ACCOUNT_DELETE_ALERT_AFTER", "20s"),
];

/// The hook positions of the v1.0 production order (06, 02, 03, 08, 05, 07a). 20 (vault) joins before 02 in v1.2
/// builds only (E-30/c05); no stand-in is built for it (AZ-3825 flagged concern).
pub const HOOK_NAMES: [&str; 6] = ["06", "02", "03", "08", "05", "07a"];

/// The 0-based position of 08's hook in `HOOK_NAMES`, as the purger's log names it.
pub const MEDIA_POSITION: usize = 3;

const DAY: Duration = Duration::from_secs(86_400);

/// The real ports one purger instance holds, all on one pool.
pub struct Wiring {
    pub accounts: Arc<IdentityAccounts>,
    pub hooks: Vec<Arc<dyn AccountLifecycleHook>>,
    pub tree: Arc<PgTreeLifecycle>,
    pub media: Arc<MediaLifecycle>,
    pub db: Db,
}

pub struct PurgeWorld {
    pub db: TestDb,
    pub clock: Arc<dyn Clock>,
    pub settings: Arc<ApiSettings>,
    pub config: AdminConfig,
    pub blobstore: BlobstoreConfig,
    pub api: Db,
    dir: PathBuf,
}

fn blobstore_config(dir: &std::path::Path) -> BlobstoreConfig {
    let volume = dir.join("volume");
    fs::create_dir_all(&volume).expect("volume folder");
    let key_file = dir.join("master.key");
    fs::write(&key_file, [0x11u8; 32]).expect("key file");
    BlobstoreConfig {
        blob_root: Some(volume.to_string_lossy().into_owned()),
        retention: 7 * DAY,
        orphan_sweep_age: DAY,
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: 0,
        blob_free_reserve_percent: 100,
        gc_interval: Duration::from_secs(60),
        gc_batch: 200,
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

impl PurgeWorld {
    /// A fresh database and volume; every component reads `clock`, the purger measures its budget on it.
    pub async fn new(clock: Arc<dyn Clock>) -> PurgeWorld {
        let settings = load_settings(Environment::Test, &PURGER_KEYS)
            .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let dir = std::env::temp_dir().join(format!("sc-admin-purge-{}", Uuid::new_v4()));
        let blobstore = blobstore_config(&dir);
        PurgeWorld {
            db,
            clock,
            config: AdminConfig::from_settings(&settings),
            settings,
            blobstore,
            api,
            dir,
        }
    }

    pub fn volume(&self) -> PathBuf {
        self.dir.join("volume")
    }

    pub async fn fixture(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// A pool of its own: a second purger instance runs on separate connections.
    pub async fn new_pool(&self) -> Db {
        self.db.api().await
    }

    /// The real hooks in the production order on `db`, with 01's account admin on the same pool.
    pub async fn wiring(&self, db: Db) -> Wiring {
        let clock = self.clock.clone();
        let audit = AuditWriter::new(db.clone(), clock.clone());
        let keyring = Arc::new(MasterKeyring::new(
            self.blobstore.clone(),
            db.clone(),
            audit.clone(),
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        let reader = Arc::new(
            FileBlobReader::new(
                &self.blobstore,
                keyring.clone(),
                db.clone(),
                audit,
                clock.clone(),
            )
            .expect("reader"),
        );
        let writer = Arc::new(
            FileBlobWriter::new(&self.blobstore, keyring, SyncBarrier::new(Duration::ZERO))
                .expect("writer"),
        );
        let gc = Arc::new(GcQueue::new(&self.blobstore, db.clone(), clock.clone()).expect("gc"));
        let quota_config = QuotaConfig::from_settings(&self.settings);
        let quota = Arc::new(PgQuotaAdmin::new(db.clone(), quota_config));
        let accounts = Arc::new(IdentityAccounts::new(db.clone(), clock.clone()));
        let commit = Arc::new(PgTreeService::new(
            db.clone(),
            accounts.clone(),
            Arc::new(PgQuotaLedger::new(quota_config)),
            gc.clone(),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            clock.clone(),
        ));
        let upload = Arc::new(PgUploadService::new(
            db.clone(),
            TransferConfig::from_settings(&self.settings),
            UploadPorts {
                guard: accounts.clone(),
                tree: Arc::new(PgTreeReader::new(
                    db.clone(),
                    &TreeConfig::from_settings(&self.settings),
                )),
                commit,
                reader,
                commit_effects: Vec::new(),
                quota: Arc::new(PgQuotaLedger::new(quota_config)),
                quota_admin: quota.clone(),
                writer,
                registry: Arc::new(
                    BlobRegistry::new(&self.blobstore, clock.clone()).expect("registry"),
                ),
                catalog: Arc::new(BlobCatalog::new(db.clone())),
            },
            clock.clone(),
        ));
        let tree = Arc::new(PgTreeLifecycle::new(db.clone(), gc.clone(), clock.clone()));
        let media = Arc::new(MediaLifecycle::new(db.clone(), gc.clone()));
        let hooks: Vec<Arc<dyn AccountLifecycleHook>> = vec![
            Arc::new(PgShareLifecycle::new(db.clone())),
            tree.clone(),
            Arc::new(PgTransferLifecycle::new(upload, gc)),
            media.clone(),
            Arc::new(PgFeedLifecycle::new(
                db.clone(),
                clock.clone(),
                Arc::new(FileEpochMarker::new(&self.blobstore).expect("epoch marker")),
            )),
            quota,
        ];
        Wiring {
            accounts,
            hooks,
            tree,
            media,
            db,
        }
    }

    /// The purger as the composition root builds it, over `accounts` and `hooks` on `db`.
    pub fn purger(
        &self,
        accounts: Arc<dyn AccountAdmin>,
        hooks: Vec<Arc<dyn AccountLifecycleHook>>,
        db: Db,
    ) -> DeletionPurger {
        DeletionPurger::new(
            accounts,
            hooks,
            self.config.clone(),
            AuditWriter::new(db.clone(), self.clock.clone()),
            db,
            self.clock.clone(),
        )
    }

    /// The real sweeper with the tree and media as its owner cleanups.
    pub fn sweeper(&self, wiring: &Wiring) -> GcSweeper {
        let tree: Arc<dyn BlobOwnerCleanup> = wiring.tree.clone();
        let media: Arc<dyn BlobOwnerCleanup> = wiring.media.clone();
        GcSweeper::new(
            &self.blobstore,
            wiring.db.clone(),
            self.clock.clone(),
            Arc::new(BlobRegistry::new(&self.blobstore, self.clock.clone()).expect("registry")),
            vec![tree, media],
            CancellationToken::new(),
        )
        .expect("sweeper")
    }

    /// The deletion request of the operator through 07's real service and 01's `begin_deletion`.
    pub async fn request_deletion(&self, account: Uuid) {
        let accounts = Arc::new(IdentityAccounts::new(self.api.clone(), self.clock.clone()));
        let service = AccountDeletionServiceImpl::new(
            accounts,
            self.config.clone(),
            AuditWriter::new(self.api.clone(), self.clock.clone()),
            self.api.clone(),
            self.clock.clone(),
        );
        service
            .request_as_operator(account)
            .await
            .expect("deletion requested");
    }

    /// Drops the database and the scratch volume.
    pub async fn finish(self) {
        let PurgeWorld { db, api, dir, .. } = self;
        drop(api);
        db.drop_db().await;
        fs::remove_dir_all(&dir).expect("remove the scratch volume");
    }
}
