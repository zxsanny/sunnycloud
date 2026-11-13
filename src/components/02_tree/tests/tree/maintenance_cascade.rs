//! AZ-3714 AC-1 (cascade in bounded batches, IT-02-4) and AC-5 (PT-02-4, one 5,000-node batch).

use std::time::{Duration, Instant};

use contracts::TreeMaintenance;

use crate::maintenance_support::{Maint, config};
use crate::support::START;

#[tokio::test]
async fn a_deleted_folder_of_100_files_cascades_in_batches_of_20_without_new_seq_values() {
    // Arrange
    let maint = Maint::new(config(20, 10)).await;
    let (tree, account) = (maint.tree(), maint.tree().account);
    let folder = tree.folder(tree.root, "F").await;
    maint.children(account, folder, "file", "f", 100).await;
    maint.delete_each(account, &[folder]).await;
    let seq = tree.seq().await;

    // Act
    let returns = maint.cascade_until_idle(START, 20).await;

    // Assert
    assert_eq!(returns, [20, 20, 20, 20, 20, 0]);
    let marked = maint
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE parent_id = '{folder}' AND deleted_at IS NOT NULL AND seq = 0"
        ))
        .await;
    assert_eq!(marked, 100, "every file marked, none stamped");
    assert_eq!(tree.seq().await, seq, "the cascade used no seq");
    let pending = maint
        .count("SELECT count(*) FROM nodes WHERE cascade_pending")
        .await;
    assert_eq!(pending, 0, "F's flag is cleared");
    maint.finish().await;
}

#[tokio::test]
async fn a_cascade_marks_subfolders_and_their_files_in_the_same_batch_and_leaves_live_siblings() {
    // Arrange
    let maint = Maint::new(config(100, 10)).await;
    let (tree, account) = (maint.tree(), maint.tree().account);
    let folder = tree.folder(tree.root, "F").await;
    let subfolders = maint.children(account, folder, "folder", "s", 3).await;
    for &sub in &subfolders {
        maint.children(account, sub, "file", "f", 5).await;
    }
    let sibling = tree.folder(tree.root, "G").await;
    maint.children(account, sibling, "file", "g", 4).await;
    maint.delete_each(account, &[folder]).await;

    // Act
    let returns = maint.cascade_until_idle(START, 100).await;

    // Assert
    assert_eq!(returns, [18, 0], "3 subfolders and 15 files in one batch");
    let live = maint
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{account}' AND deleted_at IS NULL"
        ))
        .await;
    assert_eq!(live, 1 + 1 + 4, "root, G and its files stay live");
    let pending = maint
        .count("SELECT count(*) FROM nodes WHERE cascade_pending")
        .await;
    assert_eq!(pending, 0);
    maint.finish().await;
}

/// PT-02-4 runs on REF-SRV only (`cargo test -- --ignored`): 50 subfolders of 99 files under F.
#[tokio::test]
#[ignore = "NOT-RUN(HW-REF): PT-02-4 cascade batch time is measured on REF-SRV only"]
async fn pt_02_4_one_cascade_batch_of_5000_descendants_meets_the_time_target() {
    // Arrange
    let maint = Maint::new(config(5000, 10)).await;
    let (tree, account) = (maint.tree(), maint.tree().account);
    let folder = tree.folder(tree.root, "F").await;
    for sub in maint.children(account, folder, "folder", "s", 50).await {
        maint.children(account, sub, "file", "f", 99).await;
    }
    maint.delete_each(account, &[folder]).await;
    let started = Instant::now();

    // Act
    let marked = maint
        .maintenance
        .cascade_batch(START, 5000)
        .await
        .expect("cascade batch");
    let elapsed = started.elapsed();

    // Assert
    eprintln!("PT-02-4: cascade_batch(5000) took {elapsed:?}");
    assert_eq!(marked, 5000);
    assert!(
        elapsed <= Duration::from_secs(5),
        "fail above 5 s: {elapsed:?}"
    );
    assert!(elapsed <= Duration::from_secs(2), "target 2 s: {elapsed:?}");
    maint.finish().await;
}
