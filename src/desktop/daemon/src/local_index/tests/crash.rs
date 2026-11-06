//! A killed process leaves the main file plus an unreplayed WAL. The tests copy exactly those files while the
//! store is still open and open the copy, which is what a restart after a kill sees.

use std::fs;
use std::path::Path;

use sync_core::{BatchRecord, CheckpointStore, UploadCheckpoint, UploadState};
use sync_core_proto::{BLOCK_SIZE, BatchEntry, FileHash, Hash32};
use uuid::Uuid;

use super::{entry, new_pair, open};
use crate::local_index::{INDEX_FILE_NAME, LocalIndexStore};

fn copy_as_after_a_kill(from: &Path, to: &Path) {
    let wal = format!("{INDEX_FILE_NAME}-wal");
    for name in [INDEX_FILE_NAME, wal.as_str()] {
        fs::copy(from.join(name), to.join(name)).expect("copy index file");
    }
    assert!(
        fs::metadata(to.join(&wal)).expect("wal").len() > 0,
        "writes are still in the WAL"
    );
}

#[test]
fn a_batch_record_saved_before_the_first_stage_survives_a_kill_and_is_cleared_after_the_results_are_stored()
 {
    // Arrange
    let fixture = open();
    let record = BatchRecord {
        commit_id: Uuid::from_u128(0xC0FFEE),
        entries: (0..3u128)
            .map(|n| BatchEntry {
                staged_key: Uuid::from_u128(n + 100),
                parent: Uuid::from_u128(1),
                name: format!("file-{n}.txt"),
                base_revision: None,
                file_hash: FileHash::from_bytes([n as u8; 32]),
                client_meta: None,
                import_hint: None,
            })
            .collect(),
    };
    fixture.store.save_batch(record.clone()).expect("save");
    let restart = tempfile::tempdir().expect("temp dir");

    // Act
    copy_as_after_a_kill(fixture.dir.path(), restart.path());
    let restarted = LocalIndexStore::open(restart.path()).expect("open after the kill");
    let loaded = restarted.load_batches().expect("load");
    restarted.clear_batch(record.commit_id).expect("clear");

    // Assert
    assert_eq!(loaded, vec![record]);
    assert!(restarted.load_batches().expect("load").is_empty());
    drop(restarted);
    let again = LocalIndexStore::open(restart.path()).expect("reopen");
    assert!(again.load_batches().expect("load").is_empty());
}

#[test]
fn a_large_upload_checkpoint_saved_every_32_blocks_resumes_after_a_kill_at_40_percent_and_is_gone_after_commit()
 {
    // Arrange
    let fixture = open();
    let total_blocks = 5 * 1024 * 1024 * 1024 / BLOCK_SIZE;
    let killed_at = total_blocks * 40 / 100;
    let hashes: Vec<Hash32> = (0..killed_at)
        .map(|i| Hash32::from_bytes([(i % 251) as u8; 32]))
        .collect();
    let commit_id = Uuid::from_u128(77);
    let mut last_saved = 0;
    for hashed in (32..=killed_at).step_by(32) {
        fixture
            .store
            .save(
                "pair-1:big.iso",
                UploadCheckpoint {
                    session_id: None,
                    block_hashes: hashes[..hashed].to_vec(),
                    file_hash: None,
                    commit_id,
                    state: UploadState::Hashing,
                },
            )
            .expect("save");
        last_saved = hashed;
    }
    let restart = tempfile::tempdir().expect("temp dir");

    // Act
    copy_as_after_a_kill(fixture.dir.path(), restart.path());
    let restarted = LocalIndexStore::open(restart.path()).expect("open after the kill");
    let resumed = restarted
        .load("pair-1:big.iso")
        .expect("load")
        .expect("checkpoint");
    restarted
        .clear("pair-1:big.iso")
        .expect("clear after commit ack");

    // Assert
    assert_eq!(resumed.block_hashes, hashes[..last_saved]);
    assert_eq!(resumed.commit_id, commit_id);
    assert!(
        killed_at - last_saved < 32,
        "at most one interval of blocks is hashed again"
    );
    assert_eq!(restarted.load("pair-1:big.iso").expect("load"), None);
    let rows = restarted
        .read(|conn| {
            Ok(
                conn.query_row("SELECT count(*) FROM upload_checkpoint", [], |r| {
                    r.get::<_, i64>(0)
                })?,
            )
        })
        .expect("count");
    assert_eq!(rows, 0);
}

#[test]
fn entries_written_before_a_kill_are_found_after_it() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(true)).expect("pair");
    let batch: Vec<_> = (0..200)
        .map(|n| entry(pair, &format!("Dir/File-{n}.txt"), n))
        .collect();
    fixture.store.upsert_entries(&batch).expect("upsert");
    let restart = tempfile::tempdir().expect("temp dir");

    // Act
    copy_as_after_a_kill(fixture.dir.path(), restart.path());
    let restarted = LocalIndexStore::open(restart.path()).expect("open after the kill");

    // Assert
    assert_eq!(
        restarted.get_entry(pair, "dir/file-199.TXT").expect("get"),
        Some(batch[199].clone())
    );
}
