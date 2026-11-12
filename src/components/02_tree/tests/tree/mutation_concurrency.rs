//! AZ-3708 AC-5 (a move waits for the account advisory lock) and AC-9 (nothing is added under a folder whose
//! tombstone committed first), each with two real connections.

use std::time::Duration;

use contracts::TreeServiceError;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::support::node;

/// The key `sc_db` locks for an account (`DbTx::advisory_lock_account`), taken here from a plain connection.
const HOLD_ACCOUNT_LOCK: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('sunnycloud.account:' || $1::text, 0))";

const WAITING_ADVISORY: &str = "SELECT count(*) FROM pg_locks \
    WHERE locktype = 'advisory' AND NOT granted \
    AND database = (SELECT oid FROM pg_database WHERE datname = current_database())";

const POLL: Duration = Duration::from_millis(10);
const POLL_LIMIT: u32 = 500;

async fn wait_for_a_blocked_advisory_lock(tree: &Tree) {
    for _ in 0..POLL_LIMIT {
        if tree.count(WAITING_ADVISORY).await == 1 {
            return;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("no transaction waited for the account lock");
}

#[tokio::test]
async fn a_move_blocks_while_another_connection_holds_the_account_lock_then_succeeds() {
    // Arrange
    let tree = Tree::new().await;
    let a = tree.folder(tree.root, "A").await;
    let b = tree.folder(tree.root, "B").await;
    let mut holder = tree.fixture.owner_conn().await;
    let mut held = holder.begin().await.expect("begin the holder");
    sqlx::query(HOLD_ACCOUNT_LOCK)
        .bind(tree.account)
        .execute(&mut *held)
        .await
        .expect("hold the account lock");
    let seq = tree.seq().await;

    // Act
    let (moved, seq_while_blocked) = tokio::join!(
        tree.service.update_node(tree.account, b, None, Some(a)),
        async {
            wait_for_a_blocked_advisory_lock(&tree).await;
            let seq_while_blocked = tree.seq().await;
            held.commit().await.expect("release the account lock");
            seq_while_blocked
        }
    );

    // Assert
    assert_eq!(
        seq_while_blocked, seq,
        "the move had not committed while blocked"
    );
    let moved = moved.expect("the move succeeds once the lock is released");
    assert_eq!(moved.parent_id, Some(a));
    assert_eq!(tree.seq().await, seq + 1);
    close(holder).await;
    tree.finish().await;
}

/// `(F.seq, F tombstoned, children of F, highest child seq)`.
async fn folder_state(conn: &mut PgConnection, folder: Uuid) -> (i64, bool, i64, Option<i64>) {
    sqlx::query_as(
        "SELECT f.seq, f.deleted_at IS NOT NULL, \
         (SELECT count(*) FROM nodes c WHERE c.parent_id = f.id), \
         (SELECT max(c.seq) FROM nodes c WHERE c.parent_id = f.id) \
         FROM nodes f WHERE f.id = $1",
    )
    .bind(folder)
    .fetch_one(conn)
    .await
    .expect("read folder state")
}

#[tokio::test]
async fn a_create_racing_a_delete_never_leaves_a_child_added_after_the_tombstone() {
    // Arrange
    let tree = Tree::new().await;
    let mut probe = tree.fixture.owner_conn().await;
    let mut folders = Vec::new();
    for index in 0..200 {
        let folder = node(
            &mut probe,
            tree.account,
            Some(tree.root),
            "folder",
            &format!("f{index}"),
        )
        .await
        .expect("insert F");
        folders.push(folder);
    }

    // Act
    let mut runs = Vec::new();
    for &folder in &folders {
        let (create, delete) = tokio::join!(
            tree.service.create_folder(tree.account, folder, "x"),
            tree.service.delete_node(tree.account, folder, None)
        );
        runs.push((folder, create, delete));
    }

    // Assert
    for (folder, create, delete) in runs {
        delete.expect("the delete succeeds");
        let (folder_seq, tombstoned, children, child_seq) = folder_state(&mut probe, folder).await;
        assert!(tombstoned);
        match create {
            Ok(child) => {
                assert_eq!(children, 1);
                assert_eq!(child_seq, Some(child.seq));
                assert!(
                    child.seq < folder_seq,
                    "the child committed before the tombstone"
                );
            }
            Err(TreeServiceError::ParentDeleted) => assert_eq!(children, 0),
            Err(other) => panic!("unexpected outcome {other:?}"),
        }
    }
    close(probe).await;
    tree.finish().await;
}
