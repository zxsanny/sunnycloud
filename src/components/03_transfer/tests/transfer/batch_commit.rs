//! `commit_batch` (AZ-3754 AC-1, AC-3, AC-5; IT-03-8): per-entry failures, the reused commit id, the size limit
//! and failed entries re-sent under a new id. Real 02 `publish`, 04 reader and registry, 05 stamp and 07a ledger.

use contracts::{CommitServiceError, EntryError};
use sc_observability::{BuildInfo, Metrics};
use uuid::Uuid;

use crate::batch_support::{batch, committed, entry, failures};
use crate::start_support::{Rig, transfer_config};

#[tokio::test]
async fn commit_batch_commits_498_of_500_with_two_typed_entry_errors_under_one_seq() {
    // Arrange
    let rig = Rig::new().await;
    let gone = rig.folder("gone", true).await;
    let files = rig.stage_files(rig.account, "ac1", 500).await;
    let mut entries: Vec<_> = files.iter().map(|file| entry(file, rig.root)).collect();
    entries[7].name = "bad/name".to_owned();
    entries[300].parent_id = gone;
    let commit_id = Uuid::new_v4();
    let staged_bytes: i64 = files.iter().map(|file| file.data.len() as i64).sum();

    // Act
    let result = rig
        .commit_batch(batch(commit_id, entries))
        .await
        .expect("batch committed");

    // Assert
    assert_eq!(result.per_entry.len(), 500);
    assert_eq!(
        failures(&result),
        vec![(7, EntryError::InvalidName), (300, EntryError::ParentGone)]
    );
    let nodes: Vec<Uuid> = result
        .per_entry
        .iter()
        .enumerate()
        .filter(|(index, _)| ![7, 300].contains(index))
        .map(|(_, outcome)| committed(outcome).node_id)
        .collect();
    assert_eq!(nodes.len(), 498);
    assert_eq!(
        rig.distinct_seqs(&nodes).await,
        1,
        "one stamp for the batch"
    );
    assert_eq!(rig.receipt_len(rig.account, commit_id).await, Some(500));
    assert_eq!(rig.revisions().await, 498);
    let failed_bytes = (files[7].data.len() + files[300].data.len()) as i64;
    assert_eq!(
        rig.usage().await,
        (staged_bytes - failed_bytes, failed_bytes)
    );
    assert_eq!(
        rig.staged_rows(rig.account).await,
        2,
        "failed rows stay staged"
    );
    assert_eq!(rig.uploading_blobs().await, 2);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_a_reused_id_with_a_changed_entry_is_refused_and_applies_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let files = rig.stage_files(rig.account, "ac3", 6).await;
    let entries: Vec<_> = files[..5]
        .iter()
        .map(|file| entry(file, rig.root))
        .collect();
    let commit_id = Uuid::new_v4();
    rig.commit_batch(batch(commit_id, entries.clone()))
        .await
        .expect("first batch");
    let mut changed = entries;
    changed[4] = entry(&files[5], rig.root);
    let before = (rig.revisions().await, rig.usage().await);

    // Act
    let result = rig.commit_batch(batch(commit_id, changed)).await;

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::CommitIdReused)),
        "{result:?}"
    );
    assert_eq!((rig.revisions().await, rig.usage().await), before);
    assert_eq!(
        rig.staged_rows(rig.account).await,
        1,
        "the new entry stays staged"
    );
    assert_eq!(rig.receipt_len(rig.account, commit_id).await, Some(5));
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_more_entries_than_batch_max_entries_is_too_large_and_applies_nothing() {
    // Arrange
    let mut config = transfer_config();
    config.batch_max_entries = 500;
    let rig = Rig::with_config(config).await;
    let files = rig.stage_files(rig.account, "ac3", 1).await;
    let mut entries = vec![entry(&files[0], rig.root)];
    for index in 1..501 {
        let mut other = entry(&files[0], rig.root);
        other.staged_key = Uuid::new_v4();
        other.name = format!("missing-{index}.bin");
        entries.push(other);
    }
    let commit_id = Uuid::new_v4();

    // Act
    let result = rig.commit_batch(batch(commit_id, entries)).await;

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::BatchTooLarge)),
        "{result:?}"
    );
    assert_eq!(rig.revisions().await, 0);
    assert_eq!(rig.receipts().await, 0);
    assert_eq!(rig.staged_rows(rig.account).await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_failed_entries_fixed_and_resent_under_a_new_id_commit_once() {
    // Arrange
    let rig = Rig::new().await;
    let gone = rig.folder("gone", true).await;
    let files = rig.stage_files(rig.account, "ac5", 10).await;
    let mut entries: Vec<_> = files.iter().map(|file| entry(file, rig.root)).collect();
    entries[2].name = "bad/name".to_owned();
    entries[6].parent_id = gone;
    let first = rig
        .commit_batch(batch(Uuid::new_v4(), entries))
        .await
        .expect("first batch");
    assert_eq!(failures(&first).len(), 2);
    let fixed = vec![entry(&files[2], rig.root), entry(&files[6], rig.root)];

    // Act
    let second = rig
        .commit_batch(batch(Uuid::new_v4(), fixed))
        .await
        .expect("second batch");

    // Assert
    assert!(failures(&second).is_empty(), "{second:?}");
    let names = [
        rig.node_row(committed(&second.per_entry[0]).node_id)
            .await
            .0,
        rig.node_row(committed(&second.per_entry[1]).node_id)
            .await
            .0,
    ];
    assert_eq!(names, [files[2].name.clone(), files[6].name.clone()]);
    assert_eq!(rig.revisions().await, 10);
    assert_eq!(rig.staged_rows(rig.account).await, 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_counts_batch_commits_by_result() {
    // Arrange
    let rig = Rig::new().await;
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let service = rig
        .instance()
        .with_metrics(&metrics)
        .expect("upload series");
    let files = rig.stage_files(rig.account, "metrics", 2).await;
    let entries: Vec<_> = files.iter().map(|file| entry(file, rig.root)).collect();
    let commit_id = Uuid::new_v4();

    // Act
    service
        .commit_batch(rig.actor(), batch(commit_id, entries.clone()))
        .await
        .expect("committed");
    let reused = service
        .commit_batch(rig.actor(), batch(commit_id, entries[..1].to_vec()))
        .await;
    let rendered = metrics.render().await.expect("render");

    // Assert
    assert!(matches!(reused, Err(CommitServiceError::CommitIdReused)));
    assert!(
        rendered.contains("commits_total{kind=\"batch\",result=\"committed\"} 1"),
        "{rendered}"
    );
    assert!(
        rendered.contains("commits_total{kind=\"batch\",result=\"refused\"} 1"),
        "{rendered}"
    );
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_the_same_content_on_an_existing_file_is_no_change_and_leaves_no_staged_row() {
    // Arrange
    let rig = Rig::new().await;
    let files = rig.stage_files(rig.account, "same", 2).await;
    let first = rig
        .commit_batch(batch(Uuid::new_v4(), vec![entry(&files[1], rig.root)]))
        .await
        .expect("first");
    let existing = committed(&first.per_entry[0]);
    let again = crate::batch_support::StagedFile {
        key: Uuid::new_v4(),
        name: files[1].name.clone(),
        data: files[1].data.clone(),
    };
    rig.stage_for(rig.account, &again).await;
    let mut resend = entry(&again, rig.root);
    resend.base_revision = Some(existing.revision_id);
    let files_before = rig.files();

    // Act
    let result = rig
        .commit_batch(batch(
            Uuid::new_v4(),
            vec![resend, entry(&files[0], rig.root)],
        ))
        .await
        .expect("second");

    // Assert
    assert_eq!(committed(&result.per_entry[0]), existing);
    assert!(failures(&result).is_empty());
    assert_eq!(rig.revisions().await, 2);
    assert_eq!(rig.staged_rows(rig.account).await, 0);
    assert_eq!(rig.uploading_blobs().await, 0);
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(
        rig.files(),
        files_before - 1,
        "the unused file is discarded"
    );
    rig.finish().await;
}
