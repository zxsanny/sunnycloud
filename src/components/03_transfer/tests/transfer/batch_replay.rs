//! `commit_batch` receipts (AZ-3754 AC-2, AC-4, AC-6, AC-7; IT-03-8, AT-03-2): an identical resend replays the
//! stored response, concurrent identical batches apply once without a unique violation, and receipts are per
//! account.

use std::sync::Arc;

use contracts::{CommitActor, CommitService};
use uuid::Uuid;

use crate::batch_support::{batch, committed, entry, failures};
use crate::commit_support::DEVICE;
use crate::start_support::Rig;

#[tokio::test]
async fn commit_batch_an_identical_resend_returns_the_stored_response_and_applies_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let files = rig.stage_files(rig.account, "ac2", 20).await;
    let request = batch(
        Uuid::new_v4(),
        files.iter().map(|file| entry(file, rig.root)).collect(),
    );
    let first = rig.commit_batch(request.clone()).await.expect("first");
    let before = (
        rig.nodes_of(rig.account).await,
        rig.revisions().await,
        rig.usage().await,
    );

    // Act
    let resent = rig.commit_batch(request).await.expect("resent");

    // Assert
    assert_eq!(resent, first);
    assert_eq!(
        (
            rig.nodes_of(rig.account).await,
            rig.revisions().await,
            rig.usage().await
        ),
        before
    );
    assert_eq!(rig.receipts().await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_a_batch_held_before_publish_replays_what_an_identical_one_applied() {
    // Arrange
    let rig = Rig::new().await;
    let files = rig.stage_files(rig.account, "ac4", 30).await;
    let request = batch(
        Uuid::new_v4(),
        files.iter().map(|file| entry(file, rig.root)).collect(),
    );
    let (reached, resume) = rig.commit.pause_next();
    let service = Arc::clone(&rig.service);
    let actor = rig.actor();
    let held_request = request.clone();
    let held = tokio::spawn(async move {
        let service: &dyn CommitService = service.as_ref();
        service.commit_batch(actor, held_request).await
    });
    reached.await.expect("held batch passed its checks");

    // Act
    let applied = rig.commit_batch(request).await.expect("applied");
    resume.send(()).expect("held batch waits");
    let replayed = held.await.expect("task").expect("held batch answered");

    // Assert
    assert_eq!(replayed, applied);
    assert!(failures(&applied).is_empty());
    assert_eq!(rig.revisions().await, 30);
    assert_eq!(rig.staged_rows(rig.account).await, 0);
    assert_eq!(rig.receipts().await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_two_identical_batches_at_once_apply_exactly_once() {
    // Arrange
    let rig = Rig::new().await;
    let files = rig.stage_files(rig.account, "ac4", 50).await;
    let request = batch(
        Uuid::new_v4(),
        files.iter().map(|file| entry(file, rig.root)).collect(),
    );
    let staged_bytes: i64 = files.iter().map(|file| file.data.len() as i64).sum();

    // Act
    let (one, two) = tokio::join!(rig.commit_batch(request.clone()), rig.commit_batch(request));

    // Assert
    let (one, two) = (one.expect("first answer"), two.expect("second answer"));
    assert_eq!(one, two);
    assert!(failures(&one).is_empty());
    assert_eq!(rig.revisions().await, 50);
    assert_eq!(rig.nodes_of(rig.account).await, 51, "root and 50 files");
    assert_eq!(rig.usage().await, (staged_bytes, 0));
    assert_eq!(rig.receipts().await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_receipts_are_per_account() {
    // Arrange
    let rig = Rig::new().await;
    let (other, other_root) = rig.other_account().await;
    let commit_id = Uuid::new_v4();
    let mine = rig.stage_files(rig.account, "a", 3).await;
    let theirs = rig.stage_files(other, "b", 3).await;
    let stored = rig
        .commit_batch(batch(
            commit_id,
            mine.iter().map(|file| entry(file, rig.root)).collect(),
        ))
        .await
        .expect("account A");

    // Act
    let result = rig
        .commit_batch_as(
            other,
            batch(
                commit_id,
                theirs.iter().map(|file| entry(file, other_root)).collect(),
            ),
        )
        .await
        .expect("account B");

    // Assert
    assert_ne!(result, stored);
    assert!(failures(&result).is_empty());
    assert_eq!(rig.revisions_of(other).await, 3);
    assert_eq!(rig.staged_rows(other).await, 0);
    assert_eq!(rig.receipt_len(rig.account, commit_id).await, Some(3));
    assert_eq!(rig.receipt_len(other, commit_id).await, Some(3));
    for outcome in &result.per_entry {
        let node = committed(outcome).node_id;
        assert_eq!(
            rig.count(&format!(
                "SELECT count(*) FROM nodes WHERE id = '{node}' AND account_id = '{other}'"
            ))
            .await,
            1
        );
    }
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_a_retried_folder_of_500_small_files_commits_each_once() {
    // Arrange
    let rig = Rig::new().await;
    let folder = rig.folder("camera", false).await;
    let files = rig.stage_files(rig.account, "IMG", 500).await;
    let request = batch(
        Uuid::new_v4(),
        files.iter().map(|file| entry(file, folder)).collect(),
    );
    let actor = CommitActor {
        account_id: rig.account,
        device_label: DEVICE.to_owned(),
    };
    let service: &dyn CommitService = rig.service.as_ref();

    // Act
    let first = service
        .commit_batch(actor.clone(), request.clone())
        .await
        .expect("first");
    let resent = service.commit_batch(actor, request).await.expect("resent");

    // Assert
    assert_eq!(resent, first);
    assert!(failures(&first).is_empty());
    assert_eq!(rig.revisions().await, 500);
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM nodes WHERE parent_id = '{folder}' AND deleted_at IS NULL"
        ))
        .await,
        500
    );
    let committed_file = committed(&first.per_entry[1]);
    assert_eq!(
        rig.read_blob(
            rig.revision_row(committed_file.revision_id).await.blob_id,
            files[1].data.len()
        )
        .await,
        files[1].data
    );
    assert_eq!(rig.staged_rows(rig.account).await, 0);
    rig.finish().await;
}
