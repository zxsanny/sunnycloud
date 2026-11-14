//! AC-3 under concurrency (owner decision 2026-11-14): concurrent `create_file` calls on one writer cannot all pass
//! the free-space floor before any of them allocates. The floor is set to the volume's free bytes minus 100 MiB, the
//! same as a volume with 100 MiB above its floor (no loop volume here). Its own binary, so no other test of the crate
//! takes space from the scratch volume while the creations race.

#[allow(dead_code)]
#[path = "../writer/support.rs"]
mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, BlobWriter};
use sunnycloud_blobstore::{BlobHealth, FileBlobWriter, SyncBarrier};
use support::{MIB, Rig, Settings, config};
use uuid::Uuid;

const ABOVE_FLOOR: u64 = 100 * MIB as u64;
const FILE: i64 = 30 * MIB as i64;
const STARTS: usize = 10;

/// A writer on the rig's volume and keyring with `settings`; it shares no state with the rig's own writer.
fn writer_with(rig: &Rig, settings: &Settings) -> FileBlobWriter {
    let config = config(&rig.volume, &rig.scratch.0.join("master.key"), settings);
    let barrier = SyncBarrier::new(Duration::ZERO);
    FileBlobWriter::new(&config, Arc::clone(&rig.keyring), barrier).expect("writer")
}

/// Free bytes of the volume, read by 04's own health probe (the figure the writer's floor check uses).
async fn free_bytes(rig: &Rig) -> u64 {
    let config = config(
        &rig.volume,
        &rig.scratch.0.join("master.key"),
        &Settings::default(),
    );
    let health = BlobHealth::new(&config, rig.api.clone()).expect("health");
    health.probe().await.expect("probe").free_bytes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_preallocated_creates_never_take_the_volume_below_its_floor() {
    // Arrange
    let rig = Rig::new().await;
    let floor = free_bytes(&rig).await - ABOVE_FLOOR;
    let writer = Arc::new(writer_with(
        &rig,
        &Settings {
            reserve: floor,
            io_threads: Some(STARTS as u64),
            ..Settings::default()
        },
    ));

    // Act
    let mut tasks = Vec::new();
    for _ in 0..STARTS {
        let writer = Arc::clone(&writer);
        tasks.push(tokio::spawn(async move {
            let plan = writer.plan(Uuid::new_v4(), FILE).expect("plan");
            writer.create_file(plan, FILE, true).await
        }));
    }
    let mut admitted = 0;
    let mut refused = Vec::new();
    for task in tasks {
        match task.await.expect("create task") {
            Ok(_) => admitted += 1,
            Err(error) => refused.push(error),
        }
    }
    let free = free_bytes(&rig).await;

    // Assert
    assert_eq!(
        admitted, 3,
        "100 MiB above the floor holds three 30 MiB files"
    );
    assert!(
        refused
            .iter()
            .all(|error| matches!(error, BlobError::FreeSpaceFloor)),
        "{refused:?}"
    );
    assert!(free >= floor, "free {free} below the floor {floor}");
    assert_eq!(rig.blob_files().len(), 3, "a refused create leaves no file");
    drop(writer);
    rig.finish().await;
}
