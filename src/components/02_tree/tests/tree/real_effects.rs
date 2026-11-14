//! AZ-3720 AC-2 (IT-02-10, NFT-RES-35; contract cases order-and-tail and replay-under-lock) and AC-4 (contract
//! case media-job-rollback): `publish` driven by 03's `commit_batch` and `commit_upload`, so it runs the real
//! `ReceiptEffect`, `StagedCommitEffect` or `BlobCommitEffect` and 08's `MediaEnqueueEffect`, under the real 01
//! guard. The real effects mark nothing in the log; their statements are known by the tables they touch.

use contracts::{BatchEntry, BatchEntryResult, EntryError};
use uuid::Uuid;

use crate::real_ports::{Fault, file_hash, jpeg};
use crate::real_support::Real;
use sc_testdb::statements::StatementLog;

/// The tables only the 03 and 08 effects touch inside `publish`.
const EFFECT_TABLES: [&str; 4] = ["commit_receipts", "upload_sessions", "jobs", "derived"];

fn entry(key: Uuid, parent: Uuid, name: &str) -> BatchEntry {
    BatchEntry {
        staged_key: key,
        parent_id: parent,
        name: name.to_owned(),
        base_revision: None,
        client_mtime: None,
        client_meta: None,
        import_hint: false,
    }
}

fn first(log: &[String], from: usize, what: &str, needle: &str) -> usize {
    log.iter()
        .skip(from)
        .position(|line| line.contains(needle))
        .map(|index| index + from)
        .unwrap_or_else(|| panic!("no {what} after {from} in {log:#?}"))
}

fn last(log: &[String], what: &str, needle: &str) -> usize {
    log.iter()
        .rposition(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no {what} in {log:#?}"))
}

/// The statements of the `publish` transaction, from the guard of the account lock's transaction to the end.
fn publish_transaction(log: &[String]) -> &[String] {
    let lock = last(log, "account lock", "pg_advisory_xact_lock");
    let guard = log[..lock]
        .iter()
        .rposition(|line| line.contains("FROM accounts") && line.contains("FOR SHARE"))
        .unwrap_or_else(|| panic!("no guard before the lock in {log:#?}"));
    &log[guard..]
}

fn is_effect(line: &str) -> bool {
    EFFECT_TABLES.iter().any(|table| line.contains(table))
}

#[tokio::test]
async fn a_batch_with_the_real_effects_runs_them_before_the_stamp_and_a_repeat_is_replayed() {
    // Arrange
    let real = Real::new().await;
    let files = [jpeg("one"), jpeg("two"), b"plain text".to_vec()];
    let mut entries = Vec::new();
    for (index, data) in files.iter().enumerate() {
        let key = real.stage(data).await;
        entries.push(entry(key, real.root, &format!("f{index}")));
    }
    let commit_id = Uuid::new_v4();
    let log = StatementLog::install();

    // Act
    let result = real
        .commit_batch(commit_id, entries.clone())
        .await
        .expect("commit batch");
    let statements: Vec<String> = log.take().iter().map(|s| s.trim().to_owned()).collect();
    let seq = real.seq().await;
    let repeat = real.commit_batch(commit_id, entries).await.expect("repeat");

    // Assert
    let tx = publish_transaction(&statements);
    let lock = first(tx, 0, "account lock", "pg_advisory_xact_lock");
    let receipt_lookup = first(tx, 0, "receipt lookup", "commit_receipts");
    let savepoint = first(tx, 0, "savepoint", "SAVEPOINT");
    let receipt_insert = last(tx, "receipt insert", "INSERT INTO commit_receipts");
    let stamp = last(tx, "stamp", "UPDATE account_feeds");
    let effects: Vec<usize> = (0..tx.len()).filter(|&i| is_effect(&tx[i])).collect();
    assert!(
        0 < lock && lock < receipt_lookup && receipt_lookup < savepoint,
        "{tx:#?}"
    );
    assert!(
        effects.iter().any(|&i| tx[i].contains("upload_sessions"))
            && effects.iter().any(|&i| tx[i].contains("jobs")),
        "the staged and media effects ran: {tx:#?}"
    );
    assert!(effects.iter().all(|&i| i < stamp), "{tx:#?}");
    assert!(receipt_insert < stamp, "{tx:#?}");
    assert_eq!(
        &tx[stamp + 1..],
        ["COMMIT", "notify"],
        "0 statements between the stamp and COMMIT"
    );
    assert!(
        result
            .per_entry
            .iter()
            .all(|entry| matches!(entry, BatchEntryResult::Committed(_)))
    );
    assert_eq!(real.jobs_of(&file_hash(&files[0])).await, 1);
    assert_eq!(real.jobs_of(&file_hash(&files[1])).await, 1);
    assert_eq!(real.jobs_of(&file_hash(&files[2])).await, 0);
    assert_eq!(repeat, result, "the stored response");
    assert_eq!(real.commit.replayed(), [false, true]);
    assert_eq!(real.seq().await, seq, "the replay consumed no seq");
    real.finish().await;
}

#[tokio::test]
async fn a_large_upload_runs_the_real_blob_effect_and_the_media_effect_before_the_stamp() {
    // Arrange
    let real = Real::new().await;
    let data = jpeg("large");
    let (upload_id, request) = real.upload_one("photo.jpg", &data).await;
    let log = StatementLog::install();

    // Act
    let committed = real.commit_upload(request).await.expect("commit upload");
    let statements: Vec<String> = log.take().iter().map(|s| s.trim().to_owned()).collect();

    // Assert
    let tx = publish_transaction(&statements);
    let lock = first(tx, 0, "account lock", "pg_advisory_xact_lock");
    let savepoint = first(tx, lock, "savepoint", "SAVEPOINT");
    let session = first(tx, savepoint, "session lock", "upload_sessions");
    let marked = first(tx, session, "blob marked committed", "UPDATE blobs");
    let job = first(tx, marked, "job insert", "INSERT INTO jobs");
    let release = first(tx, job, "savepoint release", "RELEASE");
    let stamp = last(tx, "stamp", "UPDATE account_feeds");
    assert!(release < stamp, "{tx:#?}");
    assert!(
        (0..tx.len())
            .filter(|&i| is_effect(&tx[i]))
            .all(|i| i < stamp)
    );
    assert_eq!(&tx[stamp + 1..], ["COMMIT", "notify"]);
    assert_eq!(real.jobs_of(&file_hash(&data)).await, 1);
    assert_eq!(
        real.count(&format!(
            "SELECT count(*) FROM upload_sessions WHERE id = '{upload_id}' AND committed_revision_id = '{}'",
            committed.revision_id
        ))
        .await,
        1,
        "the session row is the committed-key record"
    );
    real.finish().await;
}

#[tokio::test]
async fn a_jpeg_entry_whose_savepoint_fails_after_the_media_effect_leaves_no_job() {
    // Arrange
    let real = Real::new().await;
    let (kept, failed) = (jpeg("kept"), jpeg("failed"));
    let entries = vec![
        entry(real.stage(&kept).await, real.root, "kept.jpg"),
        entry(real.stage(&failed).await, real.root, "failed.jpg"),
    ];
    real.faults.set(Some(Fault::Entry(file_hash(&failed))));

    // Act
    let result = real
        .commit_batch(Uuid::new_v4(), entries)
        .await
        .expect("the batch commits");

    // Assert
    assert!(
        matches!(result.per_entry[0], BatchEntryResult::Committed(_)),
        "{result:?}"
    );
    assert_eq!(
        result.per_entry[1],
        BatchEntryResult::Failed(EntryError::UploadNotFound)
    );
    assert_eq!(real.jobs_of(&file_hash(&kept)).await, 1);
    assert_eq!(real.jobs_of(&file_hash(&failed)).await, 0);
    real.finish().await;
}
