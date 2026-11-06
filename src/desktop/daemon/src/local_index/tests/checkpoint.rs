//! The behavioural cases of sync-core's `MemoryCheckpointStore` (tests/host_traits.rs), run against the SQLite store.

use std::collections::BTreeSet;
use std::thread;

use sync_core::{BatchRecord, CheckpointStore, UploadCheckpoint, UploadState};
use sync_core_proto::{BatchEntry, FileHash, Hash32};
use uuid::Uuid;

use super::open;

fn checkpoint(seed: u8, state: UploadState) -> UploadCheckpoint {
    UploadCheckpoint {
        session_id: Some(Uuid::from_u128(u128::from(seed))),
        block_hashes: vec![
            Hash32::from_bytes([seed; 32]),
            Hash32::from_bytes([seed ^ 1; 32]),
        ],
        file_hash: Some(FileHash::from_bytes([seed; 32])),
        commit_id: Uuid::from_u128(u128::from(seed) << 8),
        state,
    }
}

fn batch(id: u128) -> BatchRecord {
    BatchRecord {
        commit_id: Uuid::from_u128(id),
        entries: vec![BatchEntry {
            staged_key: Uuid::from_u128(id + 1),
            parent: Uuid::from_u128(1),
            name: "entry ü".to_owned(),
            base_revision: Some(Uuid::from_u128(9)),
            file_hash: FileHash::from_bytes([7; 32]),
            client_meta: Some(serde_json::json!({"mode": 420})),
            import_hint: None,
        }],
    }
}

#[test]
fn a_checkpoint_round_trips_in_every_state_with_and_without_session_and_hash() {
    // Arrange
    let fixture = open();
    let hashing = UploadCheckpoint {
        session_id: None,
        block_hashes: Vec::new(),
        file_hash: None,
        commit_id: Uuid::from_u128(5),
        state: UploadState::Hashing,
    };

    // Act
    fixture
        .store
        .save("hashing", hashing.clone())
        .expect("save");
    fixture
        .store
        .save("uploading", checkpoint(2, UploadState::Uploading))
        .expect("save");
    fixture
        .store
        .save("committing", checkpoint(3, UploadState::Committing))
        .expect("save");

    // Assert
    assert_eq!(fixture.store.load("hashing").expect("load"), Some(hashing));
    assert_eq!(
        fixture.store.load("uploading").expect("load"),
        Some(checkpoint(2, UploadState::Uploading))
    );
    assert_eq!(
        fixture.store.load("committing").expect("load"),
        Some(checkpoint(3, UploadState::Committing))
    );
    assert_eq!(fixture.store.load("unknown").expect("load"), None);
}

#[test]
fn saving_again_replaces_the_checkpoint_and_clear_removes_only_that_key() {
    // Arrange
    let fixture = open();
    fixture
        .store
        .save("a", checkpoint(1, UploadState::Hashing))
        .expect("save");
    fixture
        .store
        .save("b", checkpoint(2, UploadState::Hashing))
        .expect("save");

    // Act
    fixture
        .store
        .save("a", checkpoint(3, UploadState::Uploading))
        .expect("save");
    fixture.store.clear("b").expect("clear");
    fixture
        .store
        .clear("never-saved")
        .expect("clear of a missing key");

    // Assert
    assert_eq!(
        fixture.store.load("a").expect("load"),
        Some(checkpoint(3, UploadState::Uploading))
    );
    assert_eq!(fixture.store.load("b").expect("load"), None);
}

#[test]
fn batches_come_back_in_commit_id_order_and_saving_the_same_id_replaces_it() {
    // Arrange
    let fixture = open();
    fixture.store.save_batch(batch(30)).expect("save");
    fixture.store.save_batch(batch(10)).expect("save");
    fixture.store.save_batch(batch(20)).expect("save");
    let mut replacement = batch(20);
    replacement.entries[0].name = "renamed".to_owned();

    // Act
    fixture.store.save_batch(replacement.clone()).expect("save");
    fixture
        .store
        .clear_batch(Uuid::from_u128(30))
        .expect("clear");
    fixture
        .store
        .clear_batch(Uuid::from_u128(99))
        .expect("clear of a missing batch");

    // Assert
    assert_eq!(
        fixture.store.load_batches().expect("load"),
        vec![batch(10), replacement]
    );
}

#[test]
fn eight_threads_see_their_own_last_saved_values_and_exactly_the_uncleared_batches() {
    // Arrange
    let fixture = open();
    let store = &fixture.store;
    const PER_THREAD: u128 = 50;

    // Act
    thread::scope(|scope| {
        for t in 0..8u8 {
            scope.spawn(move || {
                let key = format!("file-{t}");
                for i in 0..PER_THREAD {
                    let id = (u128::from(t) << 64) | (i << 8);
                    store.save_batch(batch(id)).expect("save batch");
                    store
                        .save(&key, checkpoint(t, UploadState::Hashing))
                        .expect("save");
                    store
                        .save(
                            &key,
                            checkpoint(t.wrapping_add(100), UploadState::Uploading),
                        )
                        .expect("save");
                    store
                        .save("shared", checkpoint(t, UploadState::Committing))
                        .expect("save");
                    assert_eq!(
                        store.load(&key).expect("load"),
                        Some(checkpoint(t.wrapping_add(100), UploadState::Uploading))
                    );
                    if i % 2 == 0 {
                        store.clear_batch(Uuid::from_u128(id)).expect("clear batch");
                    }
                    if i % 10 == 9 {
                        store.clear(&key).expect("clear");
                        assert_eq!(store.load(&key).expect("load"), None);
                    }
                }
            });
        }
    });

    // Assert
    let kept: BTreeSet<Uuid> = store
        .load_batches()
        .expect("load batches")
        .into_iter()
        .map(|b| b.commit_id)
        .collect();
    let expected: BTreeSet<Uuid> = (0..8u8)
        .flat_map(|t| {
            (0..PER_THREAD)
                .filter(|i| i % 2 == 1)
                .map(move |i| Uuid::from_u128((u128::from(t) << 64) | (i << 8)))
        })
        .collect();
    assert_eq!(kept, expected);
    assert!((0..8u8).any(
        |t| store.load("shared").expect("load") == Some(checkpoint(t, UploadState::Committing))
    ));
    for t in 0..8u8 {
        assert_eq!(store.load(&format!("file-{t}")).expect("load"), None);
    }
}

#[test]
fn a_blocklist_that_is_not_whole_hashes_is_reported_as_a_store_failure() {
    // Arrange
    let fixture = open();
    fixture
        .store
        .save("k", checkpoint(1, UploadState::Hashing))
        .expect("save");
    fixture
        .store
        .write(|conn| Ok(conn.execute("UPDATE upload_checkpoint SET blocklist = x'0102'", [])?))
        .expect("damage");

    // Act
    let result = fixture.store.load("k");

    // Assert
    assert!(result.is_err());
}
