//! AZ-3925 AC-4: the account purge in hook order 20 then 02 (owner decision 2026-11-05) for an account with a
//! vault whose root is a tree folder. Every hook reaches `remaining = 0` and no batch fails on the
//! `vaults.root_node_id` key; the tree hook run before the vault's is refused by that key, which is why the order
//! holds.

use std::sync::Arc;

use contracts::{AccountLifecycleHook, LifecycleError};
use sc_db::DbError;
use sunnycloud_vault_service::PgVaultLifecycle;
use uuid::Uuid;

use crate::lifecycle_support::{Erase, as_hook};
use crate::mutation_support::close;
use crate::support::file_with_revision;

const LIMIT: u32 = 50;
const MAX_BATCHES: usize = 20;

/// A vault rooted at a top-level folder holding one file, and one more file at the account root.
async fn account_with_a_vault(erase: &Erase) -> (Uuid, Vec<Uuid>) {
    let tree = &erase.tree;
    let vault_root = tree.folder(tree.root, "Vault").await;
    let mut owner = tree.fixture.owner_conn().await;
    let (_, _, inside) = file_with_revision(&mut owner, tree.account, vault_root, "secret").await;
    let (_, _, outside) = file_with_revision(&mut owner, tree.account, tree.root, "plain").await;
    close(owner).await;
    tree.exec(&format!(
        "INSERT INTO vaults (id, account_id, root_node_id, header, created_at, updated_at) \
         VALUES ('{}', '{}', '{vault_root}', '\\x00', 'epoch'::timestamptz, 'epoch'::timestamptz)",
        Uuid::new_v4(),
        tree.account
    ))
    .await;
    let blobs = vec![inside, outside];
    for &id in &blobs {
        erase.put_file(id);
    }
    (vault_root, blobs)
}

/// The purger's walk: each hook in order, batch after batch until it reports 0, the GC sweeper running between
/// batches as its janitor does. Returns each hook's reported `remaining` values, or the first failure.
async fn purge(
    erase: &Erase,
    hooks: &[(&'static str, Arc<dyn AccountLifecycleHook>)],
) -> Vec<(&'static str, Result<Vec<u64>, String>)> {
    let mut walk = Vec::new();
    for (name, hook) in hooks {
        let mut reported = Vec::new();
        let outcome = loop {
            if reported.len() == MAX_BATCHES {
                break Err(format!("no 0 after {MAX_BATCHES} batches: {reported:?}"));
            }
            match hook.erase_batch(erase.tree.account, LIMIT).await {
                Ok(progress) if progress.remaining == 0 => {
                    reported.push(0);
                    break Ok(reported);
                }
                Ok(progress) => {
                    reported.push(progress.remaining);
                    erase.sweep().await;
                }
                Err(error) => break Err(format!("{error:?}")),
            }
        };
        walk.push((*name, outcome));
    }
    walk
}

#[tokio::test]
async fn the_vault_hook_then_the_tree_hook_erase_everything_without_a_key_failure() {
    // Arrange
    let erase = Erase::new().await;
    let (_, blobs) = account_with_a_vault(&erase).await;
    erase.mark_deleting().await;
    let vault: Arc<dyn AccountLifecycleHook> =
        Arc::new(PgVaultLifecycle::new(erase.tree.fixture.api.clone()));
    let hooks = [("20 vault", vault), ("02 tree", as_hook(&erase.hook))];

    // Act
    let walk = purge(&erase, &hooks).await;

    // Assert
    for (name, outcome) in &walk {
        let reported = outcome
            .as_ref()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(reported.last(), Some(&0), "{name}");
    }
    for table in ["vaults", "nodes", "revisions", "blobs", "blob_gc"] {
        assert_eq!(erase.of_account(table).await, 0, "{table}");
    }
    assert!(blobs.iter().all(|&id| !erase.has_file(id)));
    drop(hooks);
    erase.finish().await;
}

#[tokio::test]
async fn the_tree_hook_before_the_vault_hook_is_refused_by_the_vault_root_key() {
    // Arrange
    let erase = Erase::new().await;
    let (vault_root, _) = account_with_a_vault(&erase).await;
    erase.mark_deleting().await;

    // Act
    let result = erase.hook.erase_batch(erase.tree.account, LIMIT).await;

    // Assert
    match result {
        Err(LifecycleError::Unexpected(fault)) => assert!(
            matches!(
                fault.downcast_ref::<DbError>(),
                Some(DbError::ForeignKeyViolation)
            ),
            "{fault:?}"
        ),
        Ok(progress) => panic!("expected the key to refuse the batch, got {progress:?}"),
    }
    assert_eq!(
        erase.of_account("revisions").await,
        2,
        "the batch rolled back"
    );
    assert_eq!(
        erase
            .tree
            .count(&format!(
                "SELECT count(*) FROM nodes WHERE id = '{vault_root}'"
            ))
            .await,
        1
    );
    erase.finish().await;
}
