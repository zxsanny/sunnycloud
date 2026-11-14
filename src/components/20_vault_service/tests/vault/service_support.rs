//! `PgVaultService` wired to the real 02 `PgTreeService` (its `VaultRoots`) on the test database, with the vault
//! section's default header cap (1024 bytes). 02 runs for real: `PgTreeService` with 05's stamp and notifier, the
//! 07a ledger, the 04 `GcQueue` and the shared `StateGuard`; `PgTreeMaintenance` with 05's `PgFeedHorizon`. The
//! service runs as `sunnycloud`; probes run as the schema owner.

use std::sync::Arc;
use std::time::Duration;

use contracts::{ETag, VaultService};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_config::Secret;
use sc_db::Db;
use sqlx::{Connection, PgConnection};
use sunnycloud_blobstore::{BlobstoreConfig, GcQueue};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedHorizon, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::{PgTreeMaintenance, PgTreeService, TreeConfig};
use sunnycloud_vault_service::{PgVaultService, VaultConfig};
use uuid::Uuid;

use crate::harness::{Harness, account_with_folder, account_with_root};

/// `max_vault_header_bytes` of these tests: the default of the vault section.
pub const MAX_HEADER: usize = 1024;

pub fn vault_config() -> VaultConfig {
    VaultConfig {
        max_vault_file_bytes: 100 * 1024 * 1024,
        max_vault_header_bytes: MAX_HEADER as u64,
    }
}

/// Short durations: a tombstone written at `START` is past the 2 s horizon at `START + 3 s`.
pub fn tree_config() -> TreeConfig {
    TreeConfig {
        tombstone_horizon: Duration::from_secs(2),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size: 1000,
        purge_batch_size: 1000,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    }
}

pub fn gc_config() -> BlobstoreConfig {
    let day = Duration::from_secs(86_400);
    BlobstoreConfig {
        blob_root: Some(std::env::temp_dir().to_string_lossy().into_owned()),
        retention: 7 * day,
        orphan_sweep_age: day,
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
        master_key_file: Secret::new("unused".to_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

/// 02's mutations and maintenance on `db`, wired as the composition root does.
pub fn tree_on(db: &Db) -> (PgTreeService, PgTreeMaintenance) {
    let clock: Arc<dyn Clock> = sc_testdb::clock_at_start();
    let queue = Arc::new(GcQueue::new(&gc_config(), db.clone(), clock.clone()).expect("gc queue"));
    let ledger = Arc::new(PgQuotaLedger::new(QuotaConfig {
        default_quota_bytes: 1 << 40,
    }));
    let service = PgTreeService::new(
        db.clone(),
        Arc::new(StateGuard::default()),
        ledger.clone(),
        queue.clone(),
        Arc::new(PgFeedStamp),
        InProcessNotifier::new(),
        clock,
    );
    let maintenance = PgTreeMaintenance::new(
        db.clone(),
        queue,
        ledger,
        Arc::new(PgFeedHorizon),
        &tree_config(),
    );
    (service, maintenance)
}

/// An account with its root, feed and quota rows (as 01 and 12 create them); returns the root.
pub async fn tree_account(conn: &mut PgConnection, account: Uuid) -> Uuid {
    let root = account_with_root(conn, account).await;
    for statement in [
        "INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)",
        "INSERT INTO quota (account_id) VALUES ($1)",
    ] {
        sqlx::query(statement)
            .bind(account)
            .execute(&mut *conn)
            .await
            .expect("insert account row");
    }
    root
}

/// A service over `db`, as the composition root builds it.
pub fn service_on(db: &Db) -> Arc<PgVaultService> {
    let (tree, _) = tree_on(db);
    let clock = sc_testdb::clock_at_start();
    Arc::new(PgVaultService::new(
        db.clone(),
        Arc::new(tree),
        clock.clone(),
        AuditWriter::new(db.clone(), clock),
        &vault_config(),
    ))
}

pub struct Env {
    pub harness: Harness,
    pub service: Arc<PgVaultService>,
    pub probe: PgConnection,
}

impl Env {
    pub async fn new() -> Env {
        let harness = Harness::new().await;
        let probe = harness.fixture_conn().await;
        let service = service_on(&harness.api);
        Env {
            harness,
            service,
            probe,
        }
    }

    /// A new account with an empty live folder; returns `(account, folder)`.
    pub async fn account(&mut self) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let folder = account_with_folder(&mut self.probe, account).await;
        (account, folder)
    }

    /// A new account with a vault holding `header`; returns `(account, vault, etag)`.
    pub async fn account_with_vault(&mut self, header: &[u8]) -> (Uuid, Uuid, ETag) {
        let (account, folder) = self.account().await;
        let created = self
            .service
            .create_vault(account, header, folder)
            .await
            .expect("create vault");
        (account, created.vault_id, created.etag)
    }

    /// The stored header bytes of `vault`, read past the service.
    pub async fn stored(&mut self, vault: Uuid) -> Option<Vec<u8>> {
        sqlx::query_scalar("SELECT header FROM vaults WHERE id = $1")
            .bind(vault)
            .fetch_optional(&mut self.probe)
            .await
            .expect("read stored header")
    }

    pub async fn vaults_of(&mut self, account: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM vaults WHERE account_id = $1")
            .bind(account)
            .fetch_one(&mut self.probe)
            .await
            .expect("count vaults")
    }

    /// The service holds a clone of the pool, so it goes first or its connections count as a leak.
    pub async fn finish(self) {
        let Env {
            harness,
            service,
            probe,
        } = self;
        drop(service);
        probe.close().await.expect("close probe connection");
        harness.finish().await;
    }
}
