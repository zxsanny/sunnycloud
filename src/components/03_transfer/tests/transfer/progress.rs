//! AC-5: four lanes setting bits and hashes for random blocks lose nothing and overwrite nothing. The rest of
//! the behaviour of the progress table: bit layout, checkpoint round trip, lazy load, refusals. The memory
//! bound (AC-6) has its own test binary, `progress_memory.rs`.

use std::sync::Arc;

use sc_ids::{Hash32, hash_block};
use sunnycloud_transfer::{
    Checkpoint, CheckpointError, ProgressError, ProgressTable, SessionProgress,
};
use uuid::Uuid;

const BLOCKS: u32 = 24;
const LANES: usize = 4;
const ROUNDS: u64 = 200;

fn hash_of(index: u32) -> Hash32 {
    hash_block(&index.to_le_bytes())
}

/// A small deterministic generator, so a failing round can be replayed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// Every block once, plus 10% of them (3 of 24) a second time, in random order.
fn plan(seed: u64) -> Vec<u32> {
    let mut random = Lcg(seed);
    let mut operations: Vec<u32> = (0..BLOCKS).collect();
    let mut repeated: Vec<u32> = Vec::new();
    while repeated.len() < 3 {
        let index = u32::try_from(random.below(u64::from(BLOCKS))).expect("index");
        if !repeated.contains(&index) {
            repeated.push(index);
        }
    }
    operations.extend(&repeated);
    for position in (1..operations.len()).rev() {
        let other = usize::try_from(random.below(position as u64 + 1)).expect("position");
        operations.swap(position, other);
    }
    operations
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_lanes_lose_no_bit_and_no_hash() {
    for seed in 0..ROUNDS {
        // Arrange
        let table = Arc::new(ProgressTable::new());
        let upload = Uuid::new_v4();
        let session = table.insert_new(upload, BLOCKS);
        let operations = plan(seed);
        let mut lanes: Vec<Vec<u32>> = vec![Vec::new(); LANES];
        for (position, index) in operations.iter().enumerate() {
            lanes[position % LANES].push(*index);
        }

        // Act
        let mut handles = Vec::new();
        for lane in lanes {
            let session = Arc::clone(&session);
            handles.push(tokio::spawn(async move {
                let mut newly_set = 0;
                for index in lane {
                    session.lock().begin_block(index).expect("begin");
                    tokio::task::yield_now().await;
                    if session
                        .lock()
                        .mark_received(index, hash_of(index))
                        .expect("mark")
                    {
                        newly_set += 1;
                    }
                }
                newly_set
            }));
        }
        let mut newly_set = 0;
        for handle in handles {
            newly_set += handle.await.expect("lane");
        }

        // Assert
        let progress = session.lock();
        assert_eq!(newly_set, BLOCKS, "seed {seed}: one first set per block");
        assert_eq!(progress.received_count(), BLOCKS, "seed {seed}");
        assert_eq!(progress.in_flight_count(), 0, "seed {seed}");
        assert!(progress.missing_blocks().is_empty(), "seed {seed}");
        for index in 0..BLOCKS {
            assert_eq!(
                progress.hash(index).expect("in range"),
                Some(hash_of(index)),
                "seed {seed} block {index}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn racing_hashes_for_one_block_keep_the_first_and_refuse_the_rest() {
    // Arrange
    let table = ProgressTable::new();
    let session = table.insert_new(Uuid::new_v4(), 1);
    let candidates: Vec<Hash32> = (0..8u32).map(hash_of).collect();

    // Act
    let mut handles = Vec::new();
    for hash in candidates.clone() {
        let session = Arc::clone(&session);
        handles.push(tokio::spawn(async move {
            session.lock().mark_received(0, hash)
        }));
    }
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.expect("lane"));
    }

    // Assert
    let winners = results.iter().filter(|result| **result == Ok(true)).count();
    let refused = results
        .iter()
        .filter(|result| **result == Err(ProgressError::HashConflict { index: 0 }))
        .count();
    let stored = session.lock().hash(0).expect("in range").expect("received");
    assert_eq!((winners, refused), (1, 7));
    assert!(candidates.contains(&stored));
}

#[test]
fn bit_i_is_bit_i_mod_8_of_byte_i_div_8_and_hashes_sit_32_bytes_apart() {
    // Arrange
    let mut progress = SessionProgress::new(20);

    // Act
    progress.mark_received(0, hash_of(0)).expect("block 0");
    progress.mark_received(9, hash_of(9)).expect("block 9");
    progress.mark_received(19, hash_of(19)).expect("block 19");

    // Assert
    assert_eq!(
        progress.received_bytes(),
        [0b0000_0001, 0b0000_0010, 0b0000_1000]
    );
    let hashes = progress.block_hash_bytes();
    assert_eq!(hashes.len(), 20 * 32);
    assert_eq!(&hashes[9 * 32..10 * 32], hash_of(9).as_bytes());
    assert_eq!(&hashes[19 * 32..], hash_of(19).as_bytes());
    assert!(hashes[32..9 * 32].iter().all(|byte| *byte == 0));
}

#[test]
fn a_repeat_with_the_same_hash_is_idempotent_and_another_hash_keeps_the_first() {
    // Arrange
    let mut progress = SessionProgress::new(4);
    progress.mark_received(2, hash_of(2)).expect("first");

    // Act
    let repeat = progress.mark_received(2, hash_of(2));
    let conflict = progress.mark_received(2, hash_of(3));

    // Assert
    assert_eq!(repeat, Ok(false));
    assert_eq!(conflict, Err(ProgressError::HashConflict { index: 2 }));
    assert_eq!(progress.hash(2), Ok(Some(hash_of(2))));
    assert_eq!(progress.received_count(), 1);
}

#[test]
fn in_flight_bits_are_set_once_and_cleared_by_ack_or_release() {
    // Arrange
    let mut progress = SessionProgress::new(10);

    // Act
    let first = progress.begin_block(3);
    let again = progress.begin_block(3);
    progress.begin_block(4).expect("block 4");
    progress.mark_received(3, hash_of(3)).expect("ack");
    progress.release_block(4).expect("release");

    // Assert
    assert_eq!((first, again), (Ok(true), Ok(false)));
    assert_eq!(progress.in_flight_count(), 0);
    assert_eq!(progress.is_in_flight(3), Ok(false));
    assert_eq!(progress.is_received(4), Ok(false));
    assert_eq!(progress.missing_blocks(), [0, 1, 2, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn an_index_past_the_block_count_is_refused_by_every_operation() {
    // Arrange
    let mut progress = SessionProgress::new(8);
    let refused = ProgressError::OutOfRange {
        index: 8,
        block_count: 8,
    };

    // Act
    let results = [
        progress.begin_block(8).map(drop),
        progress.release_block(8),
        progress.mark_received(8, hash_of(8)).map(drop),
        progress.is_received(8).map(drop),
        progress.is_in_flight(8).map(drop),
        progress.hash(8).map(drop),
    ];

    // Assert
    for result in results {
        assert_eq!(result, Err(refused));
    }
    assert_eq!(progress.received_count(), 0);
}

#[test]
fn a_session_without_blocks_is_empty_and_complete() {
    // Arrange
    let progress = SessionProgress::new(0);

    // Act
    let missing = progress.missing_blocks();

    // Assert
    assert!(missing.is_empty());
    assert!(progress.received_bytes().is_empty());
    assert!(progress.block_hash_bytes().is_empty());
    assert_eq!(
        progress.is_received(0).map(drop),
        Err(ProgressError::OutOfRange {
            index: 0,
            block_count: 0
        })
    );
}

#[test]
fn a_checkpoint_row_restores_the_received_blocks_and_nothing_in_flight() {
    // Arrange
    let mut written = SessionProgress::new(13);
    for index in [0, 5, 12] {
        written.mark_received(index, hash_of(index)).expect("mark");
    }
    written.begin_block(7).expect("in flight at the crash");
    let table = ProgressTable::new();
    let upload = Uuid::new_v4();

    // Act
    let loaded = table
        .get_or_load(
            upload,
            &Checkpoint {
                block_count: 13,
                received: written.received_bytes(),
                block_hashes: written.block_hash_bytes(),
            },
        )
        .expect("load");

    // Assert
    let progress = loaded.lock();
    assert_eq!(progress.block_count(), 13);
    assert_eq!(progress.received_bytes(), written.received_bytes());
    assert_eq!(progress.block_hash_bytes(), written.block_hash_bytes());
    assert_eq!(progress.in_flight_count(), 0);
    assert_eq!(progress.hash(5), Ok(Some(hash_of(5))));
    assert_eq!(progress.missing_blocks().len(), 10);
}

#[test]
fn an_entry_in_memory_wins_over_a_later_checkpoint_row() {
    // Arrange
    let table = ProgressTable::new();
    let upload = Uuid::new_v4();
    let live = table.insert_new(upload, 4);
    live.lock()
        .mark_received(1, hash_of(1))
        .expect("ack after the checkpoint");
    let stale = SessionProgress::new(4);

    // Act
    let found = table
        .get_or_load(
            upload,
            &Checkpoint {
                block_count: 4,
                received: stale.received_bytes(),
                block_hashes: stale.block_hash_bytes(),
            },
        )
        .expect("existing entry");

    // Assert
    assert!(Arc::ptr_eq(&found, &live));
    assert_eq!(found.lock().received_count(), 1);
    assert_eq!(table.len(), 1);
}

#[test]
fn nothing_is_loaded_until_a_session_is_touched_and_remove_drops_the_entry() {
    // Arrange
    let table = ProgressTable::new();
    let upload = Uuid::new_v4();

    // Act
    let before = (table.is_empty(), table.get(upload).is_none());
    table.insert_new(upload, 2);
    let during = (table.len(), table.get(upload).is_some());
    let removed = table.remove(upload);
    let removed_again = table.remove(upload);

    // Assert
    assert_eq!(before, (true, true));
    assert_eq!(during, (1, true));
    assert!(removed);
    assert!(!removed_again);
    assert!(table.is_empty());
    assert_eq!(table.memory_bytes(), 0);
}

#[test]
fn a_checkpoint_row_that_does_not_fit_its_block_count_is_refused() {
    // Arrange
    let hashes = vec![0u8; 10 * 32];
    let cases = [
        (
            Checkpoint {
                block_count: 10,
                received: &[0],
                block_hashes: &hashes,
            },
            CheckpointError::ReceivedLength {
                expected: 2,
                found: 1,
            },
        ),
        (
            Checkpoint {
                block_count: 10,
                received: &[0, 0],
                block_hashes: &hashes[..319],
            },
            CheckpointError::HashesLength {
                expected: 320,
                found: 319,
            },
        ),
        (
            Checkpoint {
                block_count: 10,
                received: &[0, 0b0000_0100],
                block_hashes: &hashes,
            },
            CheckpointError::BitBeyondCount,
        ),
    ];

    for (checkpoint, expected) in cases {
        // Act
        let result = SessionProgress::from_checkpoint(&checkpoint);

        // Assert
        assert_eq!(result.err(), Some(expected));
    }
}

#[test]
fn a_bitmap_filling_its_last_byte_exactly_is_accepted() {
    // Arrange
    let hashes = vec![0u8; 16 * 32];

    // Act
    let result = SessionProgress::from_checkpoint(&Checkpoint {
        block_count: 16,
        received: &[0xFF, 0xFF],
        block_hashes: &hashes,
    });

    // Assert
    let progress = result.expect("a full bitmap");
    assert_eq!(progress.received_count(), 16);
    assert!(progress.missing_blocks().is_empty());
}
