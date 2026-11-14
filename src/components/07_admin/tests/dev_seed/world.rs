//! The dev seed over the real components on a real PostgreSQL 18 and a scratch blob volume: 01's sign-in,
//! invites, operator ops, authenticator and account admin with the in-process identity provider (01's test
//! provider, standing in for mock-idp); 02's tree service and reader; 03's upload and commit service over 04's
//! writer, reader, registry and catalog, with 08's media enqueue effect in 03's commit-effect slot as the
//! composition root registers it (one instance for every account, AZ-4311); 06's share service; 07a's quota admin
//! and ledger; 05's stamp, and the account-creation hooks of 07a, 05 and 02 that the sign-up runs. Probes use the
//! schema owner.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{AccountLifecycleHook, CallbackParams, CommitEffect, Fault, async_trait};
use media_api::{JobMetrics, JobPlanner, JobQueue, MediaConfig, MediaEnqueueEffect};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_config::{Environment, Secret};
use sc_observability::{BuildInfo, Metrics};
use sc_ratelimit::{AbuseLimiter, LimitId, LimitSpec, LimitTable};
use sqlx::PgConnection;
use sunnycloud_admin::{
    AdminConfig, AdminServiceImpl, DevSeedImpl, DevSeedPorts, DevSeedTarget, ProviderLogin,
};
use sunnycloud_blobstore::{
    BlobCatalog, BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, FileEpochMarker,
    GcQueue, MasterKeyring, SyncBarrier,
};
use sunnycloud_identity::{
    IdentityAccounts, IdentityAuthenticator, IdentityConfig, IdentityInvites, IdentitySignIn,
    ProviderConfig,
};
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sharing::{PgShareService, ShareConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedLifecycle, PgFeedStamp};
use sunnycloud_transfer::{PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{PgTreeLifecycle, PgTreeReader, PgTreeService, TreeConfig};
use uuid::Uuid;

use crate::provider::FakeProvider;
use crate::support::harness::{MIGRATOR, START, TestDb};
use crate::support::settings::load_settings;

/// The provider key the seed signs in at, and the host of the test settings' public URL.
const PROVIDER: &str = "oidc";
const HOST: &str = "sunnycloud.test";
const DAY: Duration = Duration::from_secs(86_400);
const ROOMY: LimitSpec = LimitSpec::new(10_000, Duration::from_secs(60));

/// The user's step at the in-process provider: signs in as `subject`, with the subject as display name.
struct InProcessLogin(Arc<FakeProvider>);

#[async_trait]
impl ProviderLogin for InProcessLogin {
    async fn authorize(
        &self,
        authorization_url: &str,
        subject: &str,
    ) -> Result<CallbackParams, Fault> {
        let params = self.0.authorize_with(authorization_url, subject, |grant| {
            grant.name = Some(subject.to_owned());
        });
        Ok(CallbackParams(params))
    }
}

pub struct SeedWorld {
    pub db: TestDb,
    pub seed: DevSeedImpl,
    pub idp: Arc<FakeProvider>,
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

/// 08's media enqueue effect over the job queue of the test database, built as the composition root builds it.
fn media_effect(
    settings: &sc_config::ApiSettings,
    api: &sc_db::Db,
    clock: &Arc<dyn Clock>,
) -> Arc<dyn CommitEffect> {
    let config = MediaConfig::from_settings(settings);
    let registry = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let metrics = Arc::new(JobMetrics::register(&registry).expect("job metrics"));
    let queue = Arc::new(JobQueue::new(api.clone(), clock.clone(), &config, metrics));
    Arc::new(MediaEnqueueEffect::new(
        queue,
        JobPlanner::from_config(&config),
    ))
}

impl SeedWorld {
    /// The seed for `environment` over a fresh database.
    pub async fn new(environment: Environment) -> SeedWorld {
        let settings = load_settings(Environment::Test, &[])
            .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let clock: Arc<dyn Clock> = sc_testdb::clock_at_start();
        let audit = AuditWriter::new(api.clone(), clock.clone());
        let idp = Arc::new(FakeProvider::start(START.as_unix_micros() / 1_000_000).await);
        let mut identity_config = IdentityConfig::from_settings(&settings);
        identity_config.providers = vec![ProviderConfig {
            key: PROVIDER.to_owned(),
            kind: PROVIDER.to_owned(),
            issuer: Some(idp.base_url()),
            client_id: "oidc-client".to_owned(),
            client_secret: Secret::new("provider-secret".to_owned()),
        }];
        identity_config.allow_plain_http_dev = true;
        let dir = std::env::temp_dir().join(format!("sc-admin-seed-{}", Uuid::new_v4()));
        let blobstore = blobstore_config(&dir);
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
                audit.clone(),
                sc_testdb::clock_at_start(),
            )
            .expect("reader"),
        );
        let writer = Arc::new(
            FileBlobWriter::new(&blobstore, keyring, SyncBarrier::new(Duration::ZERO))
                .expect("writer"),
        );
        let gc = Arc::new(GcQueue::new(&blobstore, api.clone(), clock.clone()).expect("gc"));
        let quota_config = QuotaConfig::from_settings(&settings);
        let quota = Arc::new(PgQuotaAdmin::new(api.clone(), quota_config));
        let accounts = Arc::new(IdentityAccounts::new(api.clone(), clock.clone()));
        let tree = Arc::new(PgTreeService::new(
            api.clone(),
            accounts.clone(),
            Arc::new(PgQuotaLedger::new(quota_config)),
            gc.clone(),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            clock.clone(),
        ));
        let tree_reader = Arc::new(PgTreeReader::new(
            api.clone(),
            &TreeConfig::from_settings(&settings),
        ));
        let hooks: Vec<Arc<dyn AccountLifecycleHook>> = vec![
            quota.clone(),
            Arc::new(PgFeedLifecycle::new(
                api.clone(),
                clock.clone(),
                Arc::new(FileEpochMarker::new(&blobstore).expect("epoch marker")),
            )),
            Arc::new(PgTreeLifecycle::new(api.clone(), gc, clock.clone())),
        ];
        let limiter = Arc::new(
            AbuseLimiter::new(
                clock.clone(),
                LimitTable::new(ROOMY, ROOMY).with_spec(LimitId::SigninIp, ROOMY),
            )
            .expect("limit table"),
        );
        let sign_in = IdentitySignIn::new(
            api.clone(),
            clock.clone(),
            &identity_config,
            audit.clone(),
            limiter.clone(),
        )
        .expect("sign-in service");
        sign_in
            .register_hooks(hooks)
            .expect("hooks registered once");
        let invites = Arc::new(IdentityInvites::new(
            api.clone(),
            clock.clone(),
            &identity_config,
            audit.clone(),
        ));
        let media = media_effect(&settings, &api, &clock);
        let upload = Arc::new(PgUploadService::new(
            api.clone(),
            TransferConfig::from_settings(&settings),
            UploadPorts {
                guard: accounts.clone(),
                tree: tree_reader.clone(),
                commit: tree.clone(),
                reader,
                commit_effects: vec![media],
                quota: Arc::new(PgQuotaLedger::new(quota_config)),
                quota_admin: quota.clone(),
                writer,
                registry: Arc::new(BlobRegistry::new(&blobstore, clock.clone()).expect("registry")),
                catalog: Arc::new(BlobCatalog::new(api.clone())),
            },
            clock.clone(),
        ));
        let share_config = ShareConfig {
            grant_key: Secret::new("k".repeat(32)),
            ..ShareConfig::from_settings(&settings)
        };
        let shares = Arc::new(
            PgShareService::new(
                api.clone(),
                clock.clone(),
                tree_reader.clone(),
                audit.clone(),
                limiter,
                &share_config,
            )
            .expect("share service"),
        );
        let admin = Arc::new(AdminServiceImpl::new(
            accounts,
            quota,
            Vec::new(),
            AdminConfig::from_settings(&settings),
            audit,
            api.clone(),
        ));
        let ports = DevSeedPorts {
            operator: invites.clone(),
            invites,
            sign_in: Arc::new(sign_in),
            provider_login: Arc::new(InProcessLogin(idp.clone())),
            authenticator: Arc::new(IdentityAuthenticator::new(
                api.clone(),
                clock.clone(),
                &identity_config,
            )),
            tree,
            tree_reader,
            upload: upload.clone(),
            commit: upload,
            shares,
        };
        let target = DevSeedTarget {
            provider: PROVIDER.to_owned(),
            host: HOST.to_owned(),
        };
        let seed = DevSeedImpl::new(environment, target, admin, ports, clock);
        SeedWorld { db, seed, idp, dir }
    }

    pub async fn fixture(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Drops the database and the scratch volume.
    pub async fn finish(self) {
        let SeedWorld { db, seed, idp, dir } = self;
        drop((seed, idp));
        db.drop_db().await;
        fs::remove_dir_all(&dir).expect("remove the scratch volume");
    }
}
