//! The disk filler: the floor is set to the volume's free bytes minus 1 GiB (to 04's admission check this is the
//! same as a filler leaving floor + 1 GiB) with 04's production preallocation on; starts of 300 MiB are refused with
//! `FreeSpaceFloor` before any byte is written, blocks go into the preallocated file, and the free bytes never drop
//! below the floor. Once with starts one after another, once with ten at the same time (04 serialises its floor
//! check and allocation, owner decision 2026-11-14).

use std::sync::Arc;

use contracts::{StartUploadOutcome, UploadError};
use sc_ids::{BLOCK_SIZE, hash_block};
use sunnycloud_blobstore::BlobHealth;
use uuid::Uuid;

use crate::put_support::content;
use crate::rig_config::blob_config;
use crate::start_support::{GIB, MIB, Rig, request, transfer_config};

/// Free bytes of the volume holding `dir`, read by 04's own health probe.
async fn free_bytes(rig_db: &sc_db::Db, dir: &std::path::Path) -> u64 {
    let config = blob_config(dir, &dir.join("unused.key"), 0);
    BlobHealth::new(&config, rig_db.clone())
        .expect("health")
        .probe()
        .await
        .expect("probe")
        .free_bytes
}

/// The floor tests share the runner's temporary volume, so they take turns: one test's files would eat the other's
/// 1 GiB above the floor (the spec's dedicated small volume is not available to a component test).
static VOLUME_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A rig whose free-space floor leaves 1 GiB of the volume above it (the disk filler), with 04's production
/// preallocation on.
async fn filled_rig() -> (Rig, u64) {
    let probe = crate::support::Fixture::new().await;
    let floor = free_bytes(&probe.api, &std::env::temp_dir()).await - GIB as u64;
    probe.finish().await;
    (
        Rig::with_volume(transfer_config(), floor, true).await,
        floor,
    )
}

/// Sends the first `blocks` blocks of the session `id` and returns the lowest free bytes seen after each.
async fn send_blocks(rig: &Rig, id: Uuid, blocks: u32) -> u64 {
    let mut lowest = u64::MAX;
    for index in 0..blocks {
        let bytes = content(u64::from(index) + 950, BLOCK_SIZE);
        let (result, _) = rig
            .put_raw(id, index, hash_block(&bytes), &bytes, bytes.len() as u64)
            .await;
        result.expect("block written");
        lowest = lowest.min(free_bytes(&rig.fixture.api, &rig.volume).await);
    }
    lowest
}

const FILL: i64 = 300 * MIB;

#[tokio::test]
async fn the_free_space_floor_refuses_starts_one_after_another_and_blocks_never_cross_it() {
    // Arrange
    let _turn = VOLUME_TURN.lock().await;
    let (rig, floor) = filled_rig().await;

    // Act
    let mut admitted = Vec::new();
    let mut refused = Vec::new();
    for index in 0..10 {
        let key = format!("fill-{index}");
        match rig.start(request(rig.root, &key, FILL, &key)).await {
            Ok(StartUploadOutcome::Started(started)) => admitted.push(started.upload_id),
            Ok(other) => panic!("unexpected outcome {other:?}"),
            Err(error) => refused.push(error),
        }
    }
    let after_starts = free_bytes(&rig.fixture.api, &rig.volume).await;
    let lowest = send_blocks(&rig, admitted[0], 8).await.min(after_starts);

    // Assert
    assert_eq!(
        admitted.len(),
        3,
        "1 GiB above the floor holds three 300 MiB files"
    );
    assert!(
        refused
            .iter()
            .all(|error| matches!(error, UploadError::FreeSpaceFloor)),
        "{refused:?}"
    );
    assert!(lowest >= floor, "free {lowest} below the floor {floor}");
    assert_eq!(rig.reserved().await, 3 * FILL);
    rig.finish().await;
}

#[tokio::test]
async fn the_free_space_floor_holds_against_concurrent_starts() {
    // Arrange
    let _turn = VOLUME_TURN.lock().await;
    let (rig, floor) = filled_rig().await;

    // Act
    let mut tasks = Vec::new();
    for index in 0..10 {
        let service = Arc::clone(&rig.service);
        let (account, root, key) = (rig.account, rig.root, format!("fill-{index}"));
        tasks.push(tokio::spawn(async move {
            service
                .start_upload(account, request(root, &key, FILL, &key))
                .await
        }));
    }
    let mut admitted = Vec::new();
    let mut refused = Vec::new();
    for task in tasks {
        match task.await.expect("start task") {
            Ok(StartUploadOutcome::Started(started)) => admitted.push(started.upload_id),
            Ok(other) => panic!("unexpected outcome {other:?}"),
            Err(error) => refused.push(error),
        }
    }
    let after_starts = free_bytes(&rig.fixture.api, &rig.volume).await;
    let lowest = send_blocks(&rig, admitted[0], 8).await.min(after_starts);

    // Assert
    assert!(
        refused.iter().all(|error| matches!(
            error,
            UploadError::FreeSpaceFloor | UploadError::SessionCapReached
        )),
        "{refused:?}"
    );
    assert!(
        lowest >= floor,
        "free {lowest} below the floor {floor}: {} concurrent starts admitted",
        admitted.len()
    );
    rig.finish().await;
}
