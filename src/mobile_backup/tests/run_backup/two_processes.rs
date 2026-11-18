//! AC-78 (component part): two processes with their own checkpoint over one fixture, mutex bypassed, produce one
//! server file per asset. The real-server leg is the blackbox tier (IT-17-3).

use std::collections::BTreeSet;

use mobile_backup::Trigger;
use uuid::Uuid;

use crate::support::Rig;

const BLOCK_PLUS_ONE: usize = 2 * 1024 * 1024 + 1;

async fn back_up_together(first: &Rig, second: &Rig) {
    let (a, b) = tokio::join!(
        first.run(Trigger::AppOpen),
        second.run(Trigger::MediaChange)
    );
    assert_eq!(a, mobile_backup::BackupOutcome::Completed);
    assert_eq!(b, mobile_backup::BackupOutcome::Completed);
}

fn assert_one_file_per_asset(first: &Rig, second: &Rig, assets: usize) {
    let server = first.server.state();
    let names: BTreeSet<&String> = server.files.values().map(|file| &file.name).collect();
    assert_eq!(server.files.len(), assets, "one file per asset");
    assert_eq!(names.len(), assets, "no duplicate or conflict-copy name");
    assert_eq!(
        first.db.keys(),
        second.db.keys(),
        "both processes derive the same client_key per asset"
    );
    assert_eq!(first.db.keys().len(), assets);
    assert_eq!(first.db.count("uploaded"), assets);
    assert_eq!(second.db.count("uploaded"), assets);
}

fn assert_commit_ids_random_and_stored(first: &Rig, second: &Rig) {
    let keys: BTreeSet<Uuid> = first.db.keys().values().copied().collect();
    let mut seen = BTreeSet::new();
    for rig in [first, second] {
        for commit in &rig.api.record().commits {
            assert_eq!(commit.commit_id.get_version_num(), 7);
            assert!(
                !keys.contains(&commit.commit_id),
                "a batch id equals an asset key"
            );
            assert!(
                commit.stored_before_staging,
                "the batch record was not stored before staging"
            );
            assert!(
                seen.insert(commit.commit_id),
                "two batches share a commit id"
            );
        }
    }
    assert!(!seen.is_empty());
}

#[tokio::test]
async fn two_processes_over_100_assets_leave_exactly_100_server_files() {
    // Arrange
    let first = Rig::new(8, 50);
    let second = first.second_process();
    for id in 1..=100 {
        first.source.add(id, 1024 + id as usize);
    }

    // Act
    back_up_together(&first, &second).await;

    // Assert
    assert_one_file_per_asset(&first, &second, 100);
    assert_commit_ids_random_and_stored(&first, &second);
}

#[tokio::test]
async fn two_processes_over_300_assets_of_1_kib_to_1_mib_leave_exactly_300_server_files() {
    // Arrange
    let first = Rig::new(8, 50);
    let second = first.second_process();
    for id in 1..=300 {
        first
            .source
            .add(id, 1024 + (id as usize * 3499) % (1024 * 1024 - 1024));
    }

    // Act
    back_up_together(&first, &second).await;

    // Assert
    assert_one_file_per_asset(&first, &second, 300);
    assert_commit_ids_random_and_stored(&first, &second);
}

#[tokio::test]
async fn two_processes_over_block_path_assets_get_one_file_and_the_slower_one_nothing_to_send() {
    // Arrange
    let first = Rig::new(8, 50);
    let second = first.second_process();
    for id in 1..=10 {
        first.source.add(id, BLOCK_PLUS_ONE);
    }

    // Act
    back_up_together(&first, &second).await;

    // Assert
    assert_one_file_per_asset(&first, &second, 10);
    let server = first.server.state();
    assert_eq!((server.published, server.nothing_to_send), (10, 10));
}
