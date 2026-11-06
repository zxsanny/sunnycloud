//! AC-2, AC-3: the unique name index arbitrates names among live siblings, and the revision foreign keys
//! restrict the blob and clear the current-revision pointer. Fixtures are written as the schema owner.

use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::support::{
    Fixture, RevisionRow, account_with_root, blob, file_with_revision, node, node_row, refusal,
    revision,
};

const UNIQUE_VIOLATION: &str = "23505";
const FOREIGN_KEY_VIOLATION: &str = "23503";
const RESTRICT_VIOLATION: &str = "23001";

struct Folder {
    fixture: Fixture,
    conn: PgConnection,
    account: Uuid,
    docs: Uuid,
}

/// An account with a root and a live folder `Docs` under it.
async fn folder_docs() -> Folder {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let docs = node(&mut conn, account, Some(root), "folder", "Docs")
        .await
        .expect("insert Docs");
    Folder {
        fixture,
        conn,
        account,
        docs,
    }
}

async fn finish(folder: Folder) {
    let Folder { fixture, conn, .. } = folder;
    conn.close().await.expect("close fixture connection");
    fixture.finish().await;
}

#[tokio::test]
async fn second_live_child_with_the_same_name_key_is_a_unique_violation() {
    // Arrange
    let mut folder = folder_docs().await;
    node(
        &mut folder.conn,
        folder.account,
        Some(folder.docs),
        "file",
        "Report",
    )
    .await
    .expect("first live child");

    // Act
    let second = node_row(
        &mut folder.conn,
        Uuid::new_v4(),
        folder.account,
        Some(folder.docs),
        "file",
        "REPORT",
        "report",
    )
    .await;

    // Assert
    assert_eq!(
        refusal(second),
        (
            UNIQUE_VIOLATION.to_owned(),
            "nodes_parent_name_key".to_owned()
        )
    );
    finish(folder).await;
}

#[tokio::test]
async fn tombstoned_child_does_not_block_a_new_live_one() {
    // Arrange
    let mut folder = folder_docs().await;
    let old = node(
        &mut folder.conn,
        folder.account,
        Some(folder.docs),
        "file",
        "Report",
    )
    .await
    .expect("first live child");
    sqlx::query("UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = $1")
        .bind(old)
        .execute(&mut folder.conn)
        .await
        .expect("tombstone the child");

    // Act
    let live = node(
        &mut folder.conn,
        folder.account,
        Some(folder.docs),
        "file",
        "Report",
    )
    .await;
    let third_live = node(
        &mut folder.conn,
        folder.account,
        Some(folder.docs),
        "file",
        "Report",
    )
    .await;

    // Assert
    live.expect("a live child with the freed name");
    assert_eq!(
        refusal(third_live).0,
        UNIQUE_VIOLATION,
        "the live child blocks a third one"
    );
    finish(folder).await;
}

#[tokio::test]
async fn same_name_key_under_different_parents_is_allowed() {
    // Arrange
    let mut folder = folder_docs().await;
    let other = node(
        &mut folder.conn,
        folder.account,
        Some(folder.docs),
        "folder",
        "Other",
    )
    .await
    .expect("second folder");
    node(
        &mut folder.conn,
        folder.account,
        Some(folder.docs),
        "file",
        "Report",
    )
    .await
    .expect("child of Docs");

    // Act
    let sibling_parent = node(
        &mut folder.conn,
        folder.account,
        Some(other),
        "file",
        "Report",
    )
    .await;

    // Assert
    sibling_parent.expect("the same name under another parent");
    finish(folder).await;
}

#[tokio::test]
async fn deleting_the_blob_of_a_revision_is_refused_and_deleting_the_revision_clears_the_pointer() {
    // Arrange
    let mut folder = folder_docs().await;
    let (file, revision_id, blob_id) =
        file_with_revision(&mut folder.conn, folder.account, folder.docs, "a.txt").await;

    // Act
    let delete_blob = sqlx::query("DELETE FROM blobs WHERE id = $1")
        .bind(blob_id)
        .execute(&mut folder.conn)
        .await;
    let delete_revision = sqlx::query("DELETE FROM revisions WHERE id = $1")
        .bind(revision_id)
        .execute(&mut folder.conn)
        .await
        .expect("delete the revision");
    let pointer: Option<Uuid> =
        sqlx::query_scalar("SELECT current_revision_id FROM nodes WHERE id = $1")
            .bind(file)
            .fetch_one(&mut folder.conn)
            .await
            .expect("read the pointer");
    let blobs_left: i64 = sqlx::query_scalar("SELECT count(*) FROM blobs WHERE id = $1")
        .bind(blob_id)
        .fetch_one(&mut folder.conn)
        .await
        .expect("count the blob");

    // Assert
    assert_eq!(refusal(delete_blob).0, RESTRICT_VIOLATION);
    assert_eq!(delete_revision.rows_affected(), 1);
    assert_eq!(pointer, None);
    assert_eq!(blobs_left, 1);
    finish(folder).await;
}

#[tokio::test]
async fn deleted_revision_unblocks_the_blob() {
    // Arrange
    let mut folder = folder_docs().await;
    let (_, revision_id, blob_id) =
        file_with_revision(&mut folder.conn, folder.account, folder.docs, "a.txt").await;
    sqlx::query("DELETE FROM revisions WHERE id = $1")
        .bind(revision_id)
        .execute(&mut folder.conn)
        .await
        .expect("delete the revision");

    // Act
    let delete_blob = sqlx::query("DELETE FROM blobs WHERE id = $1")
        .bind(blob_id)
        .execute(&mut folder.conn)
        .await
        .expect("delete the blob");

    // Assert
    assert_eq!(delete_blob.rows_affected(), 1);
    finish(folder).await;
}

#[tokio::test]
async fn pointer_to_a_revision_is_checked_at_commit_not_at_the_statement() {
    // Arrange
    let mut folder = folder_docs().await;
    let (file, revision_id, blob_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    blob(&mut folder.conn, blob_id, folder.account).await;
    let mut tx = folder.conn.begin().await.expect("begin");
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, current_revision_id, created_at, updated_at) \
         VALUES ($1, $2, $3, 'file', 'b.txt', 'b.txt', 1, $4, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(file)
    .bind(folder.account)
    .bind(folder.docs)
    .bind(revision_id)
    .execute(&mut *tx)
    .await
    .expect("the pointer may lead the revision inside a transaction");
    revision(
        &mut tx,
        &RevisionRow {
            id: revision_id,
            ..RevisionRow::new(folder.account, file, blob_id)
        },
    )
    .await
    .expect("the revision follows");

    // Act
    let commit = tx.commit().await;

    // Assert
    commit.expect("the pointer resolves at commit");
    finish(folder).await;
}

#[tokio::test]
async fn pointer_to_a_missing_revision_is_refused_at_commit() {
    // Arrange
    let mut folder = folder_docs().await;
    let mut tx = folder.conn.begin().await.expect("begin");
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, current_revision_id, created_at, updated_at) \
         VALUES ($1, $2, $3, 'file', 'b.txt', 'b.txt', 1, $4, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(Uuid::new_v4())
    .bind(folder.account)
    .bind(folder.docs)
    .bind(Uuid::new_v4())
    .execute(&mut *tx)
    .await
    .expect("the statement itself passes");

    // Act
    let commit = tx.commit().await;

    // Assert
    assert_eq!(refusal(commit).0, FOREIGN_KEY_VIOLATION);
    finish(folder).await;
}
