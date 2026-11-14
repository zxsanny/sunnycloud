//! AC-8 (AZ-3923, owner decision 2026-11-13): a vault whose root folder is deleted through 02 and purged by 02's
//! tombstone purge past the horizon goes with its root (`*_cascade_vault_on_root_purge`), and the account can
//! create a new vault. 02 runs for real: `PgTreeService` with 05's stamp and notifier, the 07a ledger, the 04
//! `GcQueue` and the shared `StateGuard`; `PgTreeMaintenance` with 05's `PgFeedHorizon`. No sleep: the purge is
//! called at a time past the short horizon of `tree_config`.

use contracts::{TreeMaintenance, VaultError, VaultService};
use sc_clock::Timestamp;
use sc_testdb::START;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::harness::node;
use crate::service_support::{Env, tree_account, tree_on};

/// Past the 2 s horizon of `tree_config` for a tombstone written at `START`.
const PURGE_AT: Timestamp = Timestamp::from_unix_micros(START.as_unix_micros() + 3_000_000);
const PURGE_LIMIT: u32 = 100;
const MAX_PURGE_CALLS: usize = 10;

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
