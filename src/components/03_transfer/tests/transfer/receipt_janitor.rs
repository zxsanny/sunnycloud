//! `ReceiptJanitor` (AZ-3756 AC-3; IT-03-10): receipts older than `commit_receipt_ttl` are deleted, younger ones
//! stay, and a batch whose receipt was deleted is a new batch.

use std::sync::Arc;
use std::time::Duration;

use contracts::{EntryError, Janitor};
use sc_clock::Clock;
use sunnycloud_transfer::{ReceiptJanitor, TransferConfig};
use uuid::Uuid;

use crate::batch_support::{batch, entry, failures};
use crate::start_support::{Rig, transfer_config};

const RECEIPT_TTL: Duration = Duration::from_secs(3600);

fn config() -> TransferConfig {
    TransferConfig {
        commit_receipt_ttl: RECEIPT_TTL,
        ..transfer_config()
    }
}

#[tokio::test]
async fn receipt_janitor_deletes_only_receipts_older_than_the_ttl() {
    // Arrange
    let rig = Rig::with_config(config()).await;
    let janitor = ReceiptJanitor::new(Arc::clone(&rig.service));
    rig.clock.tick(3 * RECEIPT_TTL);
    let now = rig.clock.now_utc();
    let ttl = RECEIPT_TTL.as_micros() as i64;
    let mut kept = Vec::new();
    for index in 0..100i64 {
        let age = index * 2 * ttl / 99;
        let created = sc_clock::Timestamp::from_unix_micros(now.as_unix_micros() - age);
        let commit_id = rig.aged_receipt(created).await;
        if age <= ttl {
            kept.push(commit_id);
        }
    }

    // Act
    let cycle = janitor.run_cycle(now).await.expect("cycle");
    let again = janitor.run_cycle(now).await.expect("second cycle");

    // Assert
    assert_eq!(cycle.processed, 50);
    assert_eq!(again.processed, 0);
    assert_eq!(rig.receipts().await, 50);
    for commit_id in kept {
        assert_eq!(rig.receipt_len(rig.account, commit_id).await, Some(0));
    }
    drop(janitor);
    rig.finish().await;
}

#[tokio::test]
async fn receipt_janitor_a_replay_of_a_deleted_commit_id_is_a_new_batch() {
    // Arrange
    let rig = Rig::with_config(config()).await;
    let janitor = ReceiptJanitor::new(Arc::clone(&rig.service));
    let files = rig.stage_files(rig.account, "aged", 2).await;
    let commit_id = Uuid::new_v4();
    let request = batch(
        commit_id,
        files.iter().map(|file| entry(file, rig.root)).collect(),
    );
    let first = rig.commit_batch(request.clone()).await.expect("first");
    rig.clock.tick(RECEIPT_TTL + Duration::from_secs(1));
    let cycle = janitor.run_cycle(rig.clock.now_utc()).await.expect("cycle");

    // Act
    let replay = rig.commit_batch(request).await.expect("replay");

    // Assert
    assert_eq!(cycle.processed, 1);
    assert!(failures(&first).is_empty());
    assert_ne!(replay, first, "not the stored response");
    assert_eq!(
        failures(&replay),
        vec![
            (0, EntryError::UploadNotFound),
            (1, EntryError::UploadNotFound)
        ],
        "the staged rows were committed by the first batch"
    );
    assert_eq!(rig.receipt_len(rig.account, commit_id).await, Some(2));
    assert_eq!(rig.revisions().await, 2);
    drop(janitor);
    rig.finish().await;
}
