//! AZ-3711 AC-1 (IT-02-5, IT-02-6, IT-02-8: the delta of each `publish` outcome and the replaced blob's queue
//! row), AC-2 (IT-02-4: a file delete releases its size once), AC-3 (IT-02-4: cascade batches release exact
//! bytes) and AC-5 (a failed entry leaves usage untouched), on the real 07a ledger and admin and the real 04 queue.

use std::collections::HashMap;

use contracts::{
    EffectError, EntryResult, GcQueue, PublishOutcome, TreeMaintenance, TreeServiceError,
};
use sc_error::ErrorCode;

use crate::maintenance_support::{Maint, config};
use crate::mutation_support::Tree;
use crate::publish_support::{TestEffect, effects};
use crate::support::START;
use crate::usage_support::{KIB, MIB, RETENTION_MICROS, edit_sized, new_sized, published};

#[tokio::test]
async fn each_publish_outcome_applies_its_exact_delta_and_queues_the_replaced_blob() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(6).await;
    let (_, f, r1) = tree
        .publish_one(new_sized(tree.root, "f.bin", blobs[0], 1000))
        .await;
    let after_create = tree.view().await.used;

    // Act
    let updated = tree
        .publish_one(edit_sized(f, Some(r1), blobs[1], 3000))
        .await;
    let after_update = tree.view().await.used;
    let stale = tree
        .publish_one(edit_sized(f, Some(r1), blobs[2], 5000))
        .await;
    let after_stale = tree.view().await.used;
    tree.service
        .delete_node(tree.account, f, None)
        .await
        .expect("delete f");
    let after_delete = tree.view().await.used;
    let revive_entry = edit_sized(f, None, blobs[3], 700);
    let same_hash = revive_entry.revision.content_hash;
    let revived = tree.publish_one(revive_entry).await;
    let after_revive = tree.view().await.used;
    let mut same_entry = edit_sized(f, None, blobs[4], 700);
    same_entry.revision.content_hash = same_hash;
    let same = tree.publish_one(same_entry).await;
    let after_same = tree.view().await.used;

    // Assert
    assert_eq!(after_create, 1000, "Created: +size");
    assert_eq!(updated.0, PublishOutcome::Updated);
    assert_eq!(after_update, 3000, "Updated: +new - old");
    assert!(matches!(stale.0, PublishOutcome::ConflictCopied { .. }));
    assert_eq!(
        after_stale,
        5000 + 3000,
        "stale base: r3 plus r2 in its copy"
    );
    assert_eq!(after_delete, 3000, "the delete releases r3 only");
    assert_eq!(revived.0, PublishOutcome::Revived);
    assert_eq!(after_revive, 3000 + 700, "Revived: +new");
    assert!(matches!(same.0, PublishOutcome::NoChange { .. }));
    assert_eq!(after_same, after_revive, "NoChange: 0");
    assert_eq!(
        after_same,
        tree.live_bytes().await,
        "stored usage is live usage"
    );
    let due = START.as_unix_micros() + RETENTION_MICROS;
    assert_eq!(tree.gc_row(blobs[0]).await, Some(("replaced".into(), due)));
    assert_eq!(
        tree.gc_row(blobs[1]).await,
        None,
        "r2 is current in the copy"
    );
    assert_eq!(tree.gc_row(blobs[2]).await, Some(("deleted".into(), due)));
    assert_eq!(tree.gc_row(blobs[3]).await, None, "r4 is current");
    tree.finish().await;
}

#[tokio::test]
async fn a_file_delete_releases_its_current_size_once_and_queues_its_blob_once() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(2).await;
    let (_, f, r1) = tree
        .publish_one(new_sized(tree.root, "f.bin", blobs[0], KIB))
        .await;
    let (_, _, r2) = tree
        .publish_one(edit_sized(f, Some(r1), blobs[1], 10 * MIB))
        .await;
    let before = tree.view().await.used;
    let pending_before = tree.queue.pending_for_account(tree.account).await;

    // Act
    let stale = tree.service.delete_node(tree.account, f, Some(r1)).await;
    let after_stale = tree.view().await.used;
    tree.service
        .delete_node(tree.account, f, Some(r2))
        .await
        .expect("delete on the current base");
    let after_first = tree.view().await.used;
    tree.service
        .delete_node(tree.account, f, Some(r2))
        .await
        .expect("repeated delete");
    let after_repeat = tree.view().await.used;
    let pending_after = tree.queue.pending_for_account(tree.account).await;

    // Assert
    assert!(matches!(
        stale,
        Err(TreeServiceError::EditedSince { current_revision }) if current_revision == r2
    ));
    assert_eq!(after_stale, before, "a stale delete changes nothing");
    assert_eq!(after_first, before - 10 * MIB);
    assert_eq!(after_repeat, after_first, "the repeat releases nothing");
    assert_eq!(after_repeat, tree.live_bytes().await);
    assert_eq!(tree.queued("deleted").await, 1, "f's blob queued once");
    assert_eq!(
        tree.gc_row(blobs[1]).await.map(|(reason, _)| reason),
        Some("deleted".into())
    );
    assert_eq!(
        pending_after.expect("pending after") - pending_before.expect("pending before"),
        1
    );
    tree.finish().await;
}

#[tokio::test]
async fn cascade_batches_release_the_exact_bytes_of_the_files_they_mark() {
    // Arrange
    let maint = Maint::new(config(20, 5000)).await;
    let tree = maint.tree();
    let folder = tree
        .service
        .create_folder(tree.account, tree.root, "F")
        .await
        .expect("create F")
        .id;
    let blobs = tree.blobs(101).await;
    let entries = (0..100)
        .map(|index| new_sized(folder, &format!("f{index}"), blobs[index], KIB))
        .collect();
    let created = tree.publish_with(entries, Vec::new()).await;
    assert!(
        created
            .iter()
            .all(|r| matches!(r, EntryResult::Published { .. }))
    );
    let (_, f, _) = tree
        .publish_one(new_sized(tree.root, "f.bin", blobs[100], 10 * MIB))
        .await;
    let pending_before = maint
        .erase
        .queue
        .pending_for_account(tree.account)
        .await
        .expect("pending before");
    tree.service
        .delete_node(tree.account, f, None)
        .await
        .expect("delete f");
    tree.service
        .delete_node(tree.account, folder, None)
        .await
        .expect("delete F");
    let before = tree.view().await.used;

    // Act
    let mut returns = Vec::new();
    let mut drift = Vec::new();
    while returns.last() != Some(&0) && returns.len() < 10 {
        let marked = maint
            .maintenance
            .cascade_batch(START, 20)
            .await
            .expect("cascade batch");
        returns.push(marked);
        drift.push((tree.view().await.used, tree.live_bytes().await));
    }
    let after = tree.view().await.used;
    let pending_after = maint
        .erase
        .queue
        .pending_for_account(tree.account)
        .await
        .expect("pending after");

    // Assert
    assert_eq!(returns, [20, 20, 20, 20, 20, 0]);
    assert_eq!(before - after, 100 * KIB);
    assert!(
        drift.iter().all(|(used, live)| used == live),
        "used equals live_bytes after every batch: {drift:?}"
    );
    assert_eq!(pending_after - pending_before, 101);
    maint.finish().await;
}

#[tokio::test]
async fn a_failed_entry_leaves_used_and_reserved_bytes_untouched() {
    // Arrange: the failing entry's reservation was already released by the session reaper; another session's
    // 50 bytes stay reserved.
    let tree = Tree::new().await;
    let blobs = tree.blobs(2).await;
    let mut good = new_sized(tree.root, "good.bin", blobs[0], 300);
    good.revision.reserved_bytes = 300;
    let mut reaped = new_sized(tree.root, "reaped.bin", blobs[1], 400);
    reaped.revision.reserved_bytes = 400;
    tree.set_reserved(300 + 50).await;
    let effect = TestEffect::failing_entries(HashMap::from([(
        reaped.revision.revision_id,
        EffectError::Entry(ErrorCode::UploadSessionGone),
    )]));

    // Act
    let results = tree
        .publish_with(vec![good, reaped], effects(&effect))
        .await;

    // Assert
    assert_eq!(published(&results[0]).0, PublishOutcome::Created);
    assert!(matches!(results[1], EntryResult::Failed(_)));
    assert_eq!(tree.quota().await, (300, 50), "only the good entry counted");
    assert_eq!(tree.live_bytes().await, 300);
    tree.finish().await;
}
