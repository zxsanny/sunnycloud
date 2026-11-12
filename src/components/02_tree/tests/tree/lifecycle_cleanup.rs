//! AZ-3717 AC-1 (`initialize` inserts the one root) and AC-4 (`remove_owner_rows` removes a current, a replaced
//! and a tombstoned file's last revision through the real GC sweeper; a repeat is a no-op) — IT-02-23.

use contracts::{AccountLifecycleHook, BlobOwnerCleanup, LifecycleError};
use sc_db::{DbError, RunError, Scope};
use uuid::Uuid;

use crate::lifecycle_support::Erase;
use crate::mutation_support::close;
use crate::support::{RevisionRow, account, blob, file_with_revision, revision};

async fn initialize(erase: &Erase, account: Uuid) -> Result<(), RunError<LifecycleError>> {
    erase
        .tree
        .fixture
        .api
        .run(Scope::Account(account), async |tx| {
            erase.hook.initialize(tx, account).await
        })
        .await
}

fn refused_by(result: Result<(), RunError<LifecycleError>>) -> Option<String> {
    match result {
        Err(RunError::Closure(LifecycleError::Unexpected(fault))) => {
            match fault.downcast_ref::<DbError>() {
                Some(DbError::UniqueViolation { constraint }) => constraint.clone(),
                other => panic!("expected a unique violation, got {other:?}"),
            }
        }
        other => panic!("expected a refused root, got {other:?}"),
    }
}

#[tokio::test]
async fn initialize_inserts_one_root_and_a_second_root_is_refused() {
    // Arrange
    let erase = Erase::new().await;
    let created = Uuid::new_v4();
    let mut owner = erase.tree.fixture.owner_conn().await;
    account(&mut owner, created).await;
    close(owner).await;
    let roots = format!(
        "SELECT count(*) FROM nodes WHERE account_id = '{created}' AND parent_id IS NULL \
         AND kind = 'folder' AND name = '' AND deleted_at IS NULL"
    );

    // Act
    let first = initialize(&erase, created).await;
    let after_first = erase.tree.count(&roots).await;
    let second = initialize(&erase, created).await;

    // Assert
    assert!(first.is_ok(), "{first:?}");
    assert_eq!(after_first, 1);
    assert_eq!(refused_by(second).as_deref(), Some("nodes_root_key"));
    assert_eq!(erase.tree.count(&roots).await, 1);
    erase.finish().await;
}

/// The node's current revision, read as the superuser; `None` when the pointer is cleared.
async fn current(erase: &Erase, node: Uuid) -> Option<Uuid> {
    let mut admin = erase.tree.fixture.db.admin().await;
    let current = sqlx::query_scalar("SELECT current_revision_id FROM nodes WHERE id = $1")
        .bind(node)
        .fetch_one(&mut admin)
        .await
        .expect("read current revision");
    close(admin).await;
    current
}

async fn exists(erase: &Erase, table: &str, id: Uuid) -> bool {
    erase
        .tree
        .count(&format!("SELECT count(*) FROM {table} WHERE id = '{id}'"))
        .await
        == 1
}

#[tokio::test]
async fn the_sweeper_removes_a_current_a_replaced_and_a_tombstoned_files_revision_and_a_repeat_is_a_no_op()
 {
    // Arrange
    let erase = Erase::new().await;
    let (tree, owner_account) = (&erase.tree, erase.tree.account);
    let mut owner = tree.fixture.owner_conn().await;
    let (live, live_rev, live_blob) =
        file_with_revision(&mut owner, owner_account, tree.root, "a").await;
    let (edited, old_rev, old_blob) =
        file_with_revision(&mut owner, owner_account, tree.root, "b").await;
    let (new_rev, new_blob) = (Uuid::new_v4(), Uuid::new_v4());
    blob(&mut owner, new_blob, owner_account).await;
    revision(
        &mut owner,
        &RevisionRow {
            id: new_rev,
            ..RevisionRow::new(owner_account, edited, new_blob)
        },
    )
    .await
    .expect("insert the newer revision");
    let (gone, gone_rev, gone_blob) =
        file_with_revision(&mut owner, owner_account, tree.root, "c").await;
    close(owner).await;
    tree.exec(&format!(
        "UPDATE nodes SET current_revision_id = '{new_rev}' WHERE id = '{edited}'"
    ))
    .await;
    tree.tombstone(gone).await;
    let collected = [live_blob, old_blob, gone_blob];
    for id in collected.iter().chain([&new_blob]) {
        erase.put_file(*id);
    }
    erase.queue_now(&collected).await;

    // Act
    let removed = erase.sweep().await;
    let before_repeat = (
        erase.of_account("revisions").await,
        erase.of_account("nodes").await,
    );
    let repeat = tree
        .fixture
        .api
        .run(Scope::Account(owner_account), async |tx| {
            erase.hook.remove_owner_rows(tx, live_blob).await
        })
        .await;

    // Assert
    assert_eq!(removed, 3);
    for rev in [live_rev, old_rev, gone_rev] {
        assert!(!exists(&erase, "revisions", rev).await, "revision {rev}");
    }
    for id in collected {
        assert!(!exists(&erase, "blobs", id).await, "blob row {id}");
        assert!(!erase.has_file(id), "blob file {id}");
    }
    assert!(exists(&erase, "revisions", new_rev).await);
    assert!(exists(&erase, "blobs", new_blob).await);
    assert_eq!(current(&erase, live).await, None);
    assert_eq!(current(&erase, edited).await, Some(new_rev));
    assert_eq!(current(&erase, gone).await, None);
    assert!(exists(&erase, "nodes", gone).await);
    assert!(repeat.is_ok(), "{repeat:?}");
    assert_eq!(
        (
            erase.of_account("revisions").await,
            erase.of_account("nodes").await
        ),
        before_repeat
    );
    erase.finish().await;
}
