//! AC-6: edits within one change-token tick, and pending or trashed rows that become final later.

use mobile_backup::Trigger;
use sync_core_proto::FileHash;

use crate::support::Rig;
use crate::support::api::hash_of;

fn uploaded_hashes(rig: &Rig) -> Vec<FileHash> {
    let mut hashes: Vec<FileHash> = rig
        .server
        .state()
        .files
        .values()
        .map(|file| file.hash)
        .collect();
    hashes.sort_by_key(|hash| *hash.as_bytes());
    hashes
}

#[tokio::test]
async fn an_asset_edited_twice_in_one_tick_uploads_once_with_its_latest_hash() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 3000);
    rig.source.edit(1);
    rig.source.edit(1);
    let latest = hash_of(&rig.source.bytes_of(1));

    // Act
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert_eq!(uploaded_hashes(&rig), vec![latest]);
    assert_eq!(
        rig.db.asset(1).expect("row").content_hash,
        Some(latest.as_bytes().to_vec())
    );
}

#[tokio::test]
async fn an_uploaded_asset_that_is_edited_again_is_uploaded_again_with_the_latest_hash() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 3000);
    rig.run(Trigger::MediaChange).await;
    let first_key = rig.db.asset(1).expect("row").client_key;
    rig.source.edit(1);
    rig.source.edit(1);
    let latest = hash_of(&rig.source.bytes_of(1));

    // Act
    rig.run(Trigger::MediaChange).await;

    // Assert
    let row = rig.db.asset(1).expect("row");
    assert_eq!(rig.server.file_count(), 2, "one upload for two edits");
    assert_eq!(row.state, "uploaded");
    assert_eq!(row.content_hash, Some(latest.as_bytes().to_vec()));
    assert_ne!(row.client_key, first_key, "new content has a new key");
}

#[tokio::test]
async fn pending_and_trashed_rows_are_uploaded_only_after_they_become_final() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 2000);
    rig.source.add_hidden(2, 2000);
    rig.source.add_hidden(3, 2000);

    // Act
    rig.run(Trigger::MediaChange).await;
    let held = rig.db.states();
    rig.source.finalize(2);
    rig.source.finalize(3);
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert_eq!(
        held.keys().copied().collect::<Vec<_>>(),
        vec![1],
        "hidden rows have no row"
    );
    assert_eq!(rig.server.file_count(), 3);
    assert_eq!(rig.db.count("uploaded"), 3);
}

#[tokio::test]
async fn an_asset_gone_between_listing_and_upload_is_skipped_and_the_run_goes_on() {
    // Arrange
    let rig = Rig::new(8, 50);
    for id in 1..=3 {
        rig.source.add(id, 2000);
    }
    rig.source.vanish_on_open(2);

    // Act
    let outcome = rig.run(Trigger::MediaChange).await;

    // Assert
    assert_eq!(outcome, mobile_backup::BackupOutcome::Completed);
    assert_eq!(rig.server.file_count(), 2);
    assert_eq!(rig.db.asset(2).expect("row").state, "skipped_gone");
    assert_eq!(rig.db.count("uploaded"), 2);
}
