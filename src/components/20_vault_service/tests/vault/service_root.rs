//! Vault root and vaults per account (AZ-3923) through the real 02 `TreeReader`: AC-3 (the root must be a live,
//! empty folder of the caller) and AC-7 (one vault per account, owner decision 2026-11-11). Every refusal leaves
//! the account without a vault row.

use contracts::{VaultError, VaultService};
use uuid::Uuid;

use crate::harness::{account_with_root, node, tombstone};
use crate::service_support::Env;

#[tokio::test]
async fn foreign_unknown_tombstoned_and_file_roots_are_not_found_and_store_nothing() {
    // Arrange
    let mut env = Env::new().await;
    let (a, live_folder) = env.account().await;
    let (_, folder_of_b) = env.account().await;
    let root_of_a = node_parent(&mut env, live_folder).await;
    let tombstoned = node(&mut env.probe, a, root_of_a, "folder", "gone").await;
    tombstone(&mut env.probe, tombstoned).await;
    let gone_parent = node(&mut env.probe, a, root_of_a, "folder", "gone-parent").await;
    let under_tombstone = node(&mut env.probe, a, gone_parent, "folder", "child").await;
    tombstone(&mut env.probe, gone_parent).await;
    let file = node(&mut env.probe, a, root_of_a, "file", "file.bin").await;
    let roots = [
        ("folder of b", folder_of_b),
        ("random id", Uuid::new_v4()),
        ("tombstoned folder", tombstoned),
        ("folder under a tombstone", under_tombstone),
        ("file", file),
    ];

    // Act
    let mut outcomes = Vec::new();
    for (_, root) in roots {
        outcomes.push(env.service.create_vault(a, b"H0", root).await);
    }

    // Assert
    for ((case, _), outcome) in roots.iter().zip(outcomes) {
        assert!(
            matches!(outcome, Err(VaultError::NotFound)),
            "{case}: {outcome:?}"
        );
    }
    assert_eq!(env.vaults_of(a).await, 0);
    env.finish().await;
}

#[tokio::test]
async fn live_empty_folder_succeeds_and_a_second_vault_on_the_same_root_is_vault_exists() {
    // Arrange
    let mut env = Env::new().await;
    let (a, folder) = env.account().await;

    // Act
    let first = env.service.create_vault(a, b"H0", folder).await;
    let second = env.service.create_vault(a, b"H1", folder).await;

    // Assert
    let first = first.expect("create on a live folder");
    assert!(matches!(second, Err(VaultError::VaultExists)), "{second:?}");
    assert_eq!(env.vaults_of(a).await, 1);
    assert_eq!(env.stored(first.vault_id).await, Some(b"H0".to_vec()));
    env.finish().await;
}

#[tokio::test]
async fn a_second_vault_on_another_live_folder_is_vault_exists_and_stores_nothing() {
    // Arrange
    let mut env = Env::new().await;
    let (a, folder) = env.account().await;
    let root_of_a = node_parent(&mut env, folder).await;
    let other = node(&mut env.probe, a, root_of_a, "folder", "other").await;
    let first = env
        .service
        .create_vault(a, b"H0", folder)
        .await
        .expect("first vault");

    // Act
    let second = env.service.create_vault(a, b"H1", other).await;

    // Assert
    assert!(matches!(second, Err(VaultError::VaultExists)), "{second:?}");
    assert_eq!(env.vaults_of(a).await, 1);
    assert_eq!(env.stored(first.vault_id).await, Some(b"H0".to_vec()));
    env.finish().await;
}

#[tokio::test]
async fn a_folder_with_a_live_child_is_vault_exists_and_one_with_only_deleted_children_is_accepted()
{
    // Arrange
    let mut env = Env::new().await;
    let a = Uuid::new_v4();
    let root = account_with_root(&mut env.probe, a).await;
    let full = node(&mut env.probe, a, root, "folder", "full").await;
    node(&mut env.probe, a, full, "file", "plain.txt").await;
    let emptied = node(&mut env.probe, a, root, "folder", "emptied").await;
    let deleted_child = node(&mut env.probe, a, emptied, "file", "old.txt").await;
    tombstone(&mut env.probe, deleted_child).await;

    // Act
    let on_full = env.service.create_vault(a, b"H0", full).await;
    let on_emptied = env.service.create_vault(a, b"H0", emptied).await;

    // Assert
    assert!(
        matches!(on_full, Err(VaultError::VaultExists)),
        "{on_full:?}"
    );
    let created = on_emptied.expect("create on a folder without live children");
    assert_eq!(env.stored(created.vault_id).await, Some(b"H0".to_vec()));
    env.finish().await;
}

/// The parent of `node`, read past the service.
async fn node_parent(env: &mut Env, node: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(node)
        .fetch_one(&mut env.probe)
        .await
        .expect("read parent")
}
