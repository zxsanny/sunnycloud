//! AC-5 and AC-73: blobs whose files are gone after a restore are typed findings in every component that meets
//! them; nothing fails, nothing is deleted by mistake.

use std::sync::Arc;

use contracts::{BlobError, BlobInventory as _, ScrubMode, Scrubber};
use sc_clock::Clock;
use sunnycloud_blobstore::{BlobInventory, OrphanSweeper};
use uuid::Uuid;

use crate::fixture::open_error;
use crate::inventory::inventory;
use crate::support::{Rig, run_to_done};

async fn all_missing(inventory: &BlobInventory, rows: usize) -> Vec<Uuid> {
    let mut found = Vec::new();
    for _ in 0..rows {
        found.extend(inventory.find_missing(1).await.expect("page"));
    }
    found.sort();
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn a_report_sweep_deletes_nothing_and_the_inventory_names_the_five_missing_blobs() {
    // Arrange
    let rig = Rig::new().await;
    let mut blobs = Vec::new();
    for seed in 0..100u8 {
        blobs.push(rig.store(2048, seed).await);
    }
    let gone: Vec<Uuid> = blobs.iter().take(5).map(|blob| blob.blob_id).collect();
    for id in &gone {
        rig.delete_file(*id);
    }
    let sweeper =
        OrphanSweeper::new(&rig.config, rig.api.clone(), rig.clock.clone()).expect("sweeper");
    let inventory = inventory(&rig);

    // Act
    let swept = sweeper.run_cycle(rig.clock.now_utc()).await.expect("sweep");
    let found = all_missing(&inventory, 100).await;
    let mut opened = Vec::new();
    for blob in &blobs[..5] {
        opened.push(open_error(&rig, blob).await);
    }

    // Assert
    let mut expected = gone.clone();
    expected.sort();
    assert_eq!((swept.found, swept.removed), (0, 0));
    assert_eq!(found, expected);
    assert!(
        opened
            .iter()
            .all(|error| matches!(error, Some(BlobError::BlobMissing)))
    );
    for blob in &blobs[5..] {
        assert!(rig.path(blob.blob_id).exists());
    }
    drop((sweeper, inventory));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn three_missing_files_are_findings_for_the_inventory_the_reader_the_scrub_and_the_collector()
{
    // Arrange
    let rig = Rig::new().await;
    let mut blobs = Vec::new();
    for seed in 0..10u8 {
        blobs.push(rig.store(2048, seed).await);
    }
    let gone: Vec<Uuid> = blobs[..3].iter().map(|blob| blob.blob_id).collect();
    for id in &gone {
        rig.delete_file(*id);
    }
    let inventory = inventory(&rig);
    let scrub = rig.scrub(4);
    let sweeper = rig.sweeper(None);

    // Act
    let found = all_missing(&inventory, 10).await;
    let mut opened = Vec::new();
    for blob in &blobs[..3] {
        opened.push(open_error(&rig, blob).await);
    }
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("start run");
    let (totals, _) = run_to_done(scrub.scrubber.as_ref(), &run, 20).await;
    rig.schedule_removal(gone[0]).await;
    let swept = sweeper.run_cycle(rig.clock.now_utc()).await;

    // Assert
    let mut expected = gone.clone();
    expected.sort();
    assert_eq!(found, expected);
    assert!(
        opened
            .iter()
            .all(|error| matches!(error, Some(BlobError::BlobMissing)))
    );
    assert!(totals.done);
    assert_eq!((totals.checked, totals.missing), (10, 3));
    assert_eq!(totals.mismatched, 0);
    let stats = swept.expect("the collector does not fail on a missing file");
    assert_eq!(stats.removed, 1);
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM blobs WHERE id = '{}'",
            gone[0]
        ))
        .await,
        0,
        "the row of the collected blob is gone"
    );
    drop((inventory, scrub, sweeper, Arc::clone(&rig.keyring)));
    rig.finish().await;
}
