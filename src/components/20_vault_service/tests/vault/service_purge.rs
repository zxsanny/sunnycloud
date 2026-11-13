//! AC-8 (AZ-3923, owner decision 2026-11-13): a vault whose root folder is deleted through 02 and purged by 02's
//! tombstone purge past the horizon goes with its root (`*_cascade_vault_on_root_purge`), and the account can
//! create a new vault. 02 runs for real: `PgTreeService` with 05's stamp and notifier, the 07a ledger, the 04
//! `GcQueue` and the shared `StateGuard`; `PgTreeMaintenance` with 05's `PgFeedHorizon`. No sleep: the purge is
//! called at a time past the short horizon of `tree_config`.

use std::sync::Arc;
use std::time::Duration;

use contracts::{TreeMaintenance, VaultError, VaultService};
use sc_clock::{Clock, Timestamp};
use sc_config::Secret;
use sc_db::Db;
use sc_testdb::START;
use sqlx::PgConnection;
use sunnycloud_blobstore::{BlobstoreConfig, GcQueue};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedHorizon, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::{PgTreeMaintenance, PgTreeService};
use uuid::Uuid;

use crate::harness::{account_with_root, node};
use crate::service_support::{Env, tree_config};

/// Past the 2 s horizon of `tree_config` for a tombstone written at `START`.
const PURGE_AT: Timestamp = Timestamp::from_unix_micros(START.as_unix_micros() + 3_000_000);
const PURGE_LIMIT: u32 = 100;
const MAX_PURGE_CALLS: usize = 10;

fn gc_config() -> BlobstoreConfig {
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
fn tree_on(db: &Db) -> (PgTreeService, PgTreeMaintenance) {
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
async fn tree_account(conn: &mut PgConnection, account: Uuid) -> Uuid {
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

async fn node_exists(conn: &mut PgConnection, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("probe node")
}

#[tokio::test]
async fn purging_the_deleted_vault_root_removes_the_vault_and_a_new_vault_can_be_created() {
    // Arrange
    let mut env = Env::new().await;
    let a = Uuid::new_v4();
    let root = tree_account(&mut env.probe, a).await;
    let vault_root = node(&mut env.probe, a, root, "folder", "vault").await;
    let next_root = node(&mut env.probe, a, root, "folder", "next").await;
    let (tree, maintenance) = tree_on(&env.harness.api);
    let vault = env
        .service
        .create_vault(a, b"H0", vault_root)
        .await
        .expect("create vault");
    tree.delete_node(a, vault_root, None)
        .await
        .expect("delete the vault root through 02");

    // Act
    let mut purged = Vec::new();
    while purged.last() != Some(&0) && purged.len() < MAX_PURGE_CALLS {
        purged.push(
            maintenance
                .purge_tombstones(PURGE_AT, PURGE_LIMIT)
                .await
                .expect("tombstone purge"),
        );
    }
    let read_after = env.service.get_header(a, vault.vault_id).await;
    let recreated = env.service.create_vault(a, b"H1", next_root).await;

    // Assert
    assert_eq!(purged, [1, 0], "the root is purged in one batch");
    assert!(!node_exists(&mut env.probe, vault_root).await);
    assert_eq!(env.stored(vault.vault_id).await, None);
    assert!(
        matches!(read_after, Err(VaultError::NotFound)),
        "{read_after:?}"
    );
    let recreated = recreated.expect("a new vault after the purge");
    assert_eq!(env.stored(recreated.vault_id).await, Some(b"H1".to_vec()));
    drop((tree, maintenance));
    env.finish().await;
}
