//! AZ-3927 (batch 58 warning, closed): a child created in the instant between the emptiness check and the vault
//! insert can no longer sit in a new vault without being a member. The vault create locks the root row before it
//! checks, and every create holds that row `FOR SHARE` as its parent, so either the check sees the child (the vault
//! is refused) or the child is created after the vault and copies its membership. Both sides are the real code:
//! 20's `create_vault` and 02's `create_folder`; the held lock of the first test is a create of 02 caught between
//! its parent lock and its COMMIT.

use std::time::Duration;

use contracts::{VaultError, VaultService};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::harness::node;
use crate::service_support::{Env, tree_account, tree_on};

const ROUNDS: usize = 30;
const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(10);
const LOCK_POLL: Duration = Duration::from_millis(10);

/// The vault a node is in, read past both services.
async fn vault_of(conn: &mut PgConnection, node: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT vault_id FROM nodes WHERE id = $1")
        .bind(node)
        .fetch_one(conn)
        .await
        .expect("read vault_id")
}

/// Waits until some backend of this database waits for a lock, polling `pg_locks`.
async fn until_a_backend_waits_for_a_lock(conn: &mut PgConnection) {
    let waited = tokio::time::timeout(LOCK_WAIT_LIMIT, async {
        loop {
            // `pg_locks` and the database of a backend are visible to every role; the wait event of another
            // role's backend is not.
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity s ON s.pid = l.pid \
                 WHERE NOT l.granted AND s.datname = current_database()",
            )
            .fetch_one(&mut *conn)
            .await
            .expect("read pg_stat_activity");
            if waiting > 0 {
                return;
            }
            tokio::time::sleep(LOCK_POLL).await;
        }
    })
    .await;
    waited.expect("the vault create waits for the root lock");
}

#[tokio::test]
async fn a_child_whose_create_holds_the_root_lock_makes_the_vault_create_wait_and_then_refuse() {
    // Arrange
    let mut env = Env::new().await;
    let a = Uuid::new_v4();
    let root = tree_account(&mut env.probe, a).await;
    let folder = node(&mut env.probe, a, root, "folder", "vault").await;
    let mut creator = env.harness.fixture_conn().await;
    let mut in_flight = creator.begin().await.expect("begin the child's create");
    sqlx::query("SELECT 1 FROM nodes WHERE id = $1 FOR SHARE")
        .bind(folder)
        .execute(&mut *in_flight)
        .await
        .expect("the parent lock of a create");
    let child = node(&mut in_flight, a, folder, "file", "plain.txt").await;

    // Act
    let service = env.service.clone();
    let created = tokio::spawn(async move { service.create_vault(a, b"H0", folder).await });
    until_a_backend_waits_for_a_lock(&mut env.probe).await;
    in_flight.commit().await.expect("commit the child");
    let created = created.await.expect("create task");

    // Assert
    assert!(
        matches!(created, Err(VaultError::VaultExists)),
        "{created:?}"
    );
    assert_eq!(env.vaults_of(a).await, 0);
    assert_eq!(vault_of(&mut env.probe, child).await, None);
    assert_eq!(vault_of(&mut env.probe, folder).await, None);
    creator.close().await.expect("close creator connection");
    env.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_folder_created_while_the_vault_is_created_is_a_member_exactly_when_the_vault_was_created()
 {
    // Arrange
    let mut env = Env::new().await;
    let (tree, _) = tree_on(&env.harness.api);
    let tree = std::sync::Arc::new(tree);
    let mut cases = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let a = Uuid::new_v4();
        let root = tree_account(&mut env.probe, a).await;
        cases.push((a, node(&mut env.probe, a, root, "folder", "vault").await));
    }

    // Act
    let mut outcomes = Vec::with_capacity(ROUNDS);
    for &(a, folder) in &cases {
        let service = env.service.clone();
        let vault = tokio::spawn(async move { service.create_vault(a, b"H0", folder).await });
        let tree = tree.clone();
        let child = tokio::spawn(async move { tree.create_folder(a, folder, "child").await });
        let vault = vault.await.expect("create task");
        let child = child.await.expect("folder task").expect("create folder");
        outcomes.push((vault, child.id));
    }

    // Assert
    for (vault, child) in outcomes {
        let membership = vault_of(&mut env.probe, child).await;
        match vault {
            Ok(created) => assert_eq!(membership, Some(created.vault_id)),
            Err(VaultError::VaultExists) => assert_eq!(membership, None),
            Err(other) => panic!("unexpected create outcome: {other:?}"),
        }
    }
    drop(tree);
    env.finish().await;
}
