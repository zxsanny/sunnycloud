//! `TreeRestore::preview_restore` (AZ-3796 AC-6): the answer of the real restore with every row, count and
//! feed position left as it was, and the same refusals.

use contracts::{Placement, RestoreError};

use crate::restore_support::{Restore, hours_ago, micros_after};

#[tokio::test]
async fn a_preview_returns_what_the_restore_then_does_and_changes_nothing() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    rig.delete(file.node).await;
    let at = micros_after(hours_ago(3), 1);
    let rows = rig.snapshot().await;
    let (used, _) = rig.tree.quota().await;
    let seq = rig.tree.seq().await;
    let blob_state = rig.blob_state(file.blob1).await;

    // Act
    let preview = rig.preview_by_node(file.node, at).await.expect("preview");
    let unchanged = rig.snapshot().await == rows
        && rig.tree.quota().await.0 == used
        && rig.tree.seq().await == seq
        && rig.blob_state(file.blob1).await == blob_state;
    let restored = rig.by_node(file.node, at).await.expect("restore");

    // Assert
    assert!(unchanged, "a preview stores nothing");
    assert_eq!(preview, restored);
    assert_eq!(preview.restored_revision, file.r1);
    assert_eq!(preview.placement, Placement::Original);
    assert_ne!(
        rig.snapshot().await,
        rows,
        "the real restore does change rows"
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_preview_under_a_deleted_folder_says_restored_and_creates_no_folder() {
    // Arrange
    let rig = Restore::new().await;
    let old = rig.tree.folder(rig.tree.root, "old").await;
    let file = rig.two_revisions(old, "report.txt").await;
    rig.delete(file.node).await;
    rig.delete(old).await;
    let rows = rig.snapshot().await;

    // Act
    let preview = rig
        .preview_by_node(file.node, micros_after(hours_ago(3), 1))
        .await
        .expect("preview");

    // Assert
    assert_eq!(preview.placement, Placement::Restored);
    assert_eq!(rig.snapshot().await, rows);
    rig.finish().await;
}

#[tokio::test]
async fn a_preview_refuses_like_the_restore_and_leaves_every_row_alone() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    rig.delete(file.node).await;
    let missing_blob = rig.two_revisions(rig.tree.root, "b.txt").await;
    rig.delete(missing_blob.node).await;
    rig.drop_file(missing_blob.blob1);
    let rows = rig.snapshot().await;

    // Act
    let beyond = rig.preview_by_node(file.node, hours_ago(8 * 24)).await;
    let before_any_revision = rig
        .preview_by_node(file.node, micros_after(hours_ago(3), -1))
        .await;
    let no_file = rig
        .preview_by_node(missing_blob.node, micros_after(hours_ago(3), 1))
        .await;

    // Assert
    assert!(matches!(beyond, Err(RestoreError::BeyondRetention)));
    assert!(matches!(before_any_revision, Err(RestoreError::NotFound)));
    assert!(matches!(no_file, Err(RestoreError::FileMissing)));
    assert_eq!(rig.snapshot().await, rows);
    rig.finish().await;
}
