//! ST-04-8 (AC-43): a block index is sealed with one plaintext only. A second `write_block` of an index that
//! holds a block is accepted with identical bytes, whose stored ciphertext is written back unchanged; other bytes,
//! or a stored region that does not authenticate (a torn write), are `BlockConflict` and change no byte of the file.
//! A write is seen through the file's modification time, backdated before the second write.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use contracts::{BlobError, BlobHandle, BlobWriter};
use sunnycloud_blobstore::{
    BlobLayout, BlobstoreConfig, CHUNK_SIZE, FileBlobWriter, SyncBarrier, TAG_LEN,
};

use crate::kit::{BLOCK, block_of, commit, marked, read_all};
use crate::support::Rig;

const SIZE: usize = 2 * BLOCK + 1000;
const INDEX: u32 = 1;
/// The tags of the 32 chunks of a full block: its region is this much longer than its plaintext.
const REGION_TAGS: usize = 32 * TAG_LEN as usize;

fn writer_with(rig: &Rig, preallocate: bool) -> FileBlobWriter {
    let config = BlobstoreConfig {
        blob_preallocate: preallocate,
        ..rig.config.clone()
    };
    FileBlobWriter::new(
        &config,
        Arc::clone(&rig.keyring),
        SyncBarrier::new(Duration::ZERO),
    )
    .expect("writer")
}

async fn created(rig: &Rig, writer: &FileBlobWriter, preallocate: bool) -> BlobHandle {
    let plan = writer.plan(rig.account, SIZE as i64).expect("plan");
    writer
        .create_file(plan, SIZE as i64, preallocate)
        .await
        .expect("create_file")
}

async fn write(writer: &FileBlobWriter, handle: &BlobHandle, index: u32, bytes: Bytes) {
    let token = writer
        .write_block(handle, index, bytes)
        .await
        .expect("block is written");
    writer.await_durable(token).await.expect("block is durable");
}

/// A blob of `source` with every block written and durable.
async fn written(rig: &Rig, source: &[u8]) -> BlobHandle {
    let handle = created(rig, &rig.writer, false).await;
    for index in 0..3 {
        write(
            &rig.writer,
            &handle,
            index,
            block_of(source, index as usize),
        )
        .await;
    }
    handle
}

/// The file's modification time set far in the past, so any later `pwrite` shows as a newer time.
fn backdate(rig: &Rig, handle: &BlobHandle) -> SystemTime {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    fs::File::options()
        .write(true)
        .open(rig.path(handle.plan.blob_id))
        .expect("blob file")
        .set_modified(old)
        .expect("set mtime");
    old
}

fn modified(rig: &Rig, handle: &BlobHandle) -> SystemTime {
    fs::metadata(rig.path(handle.plan.blob_id))
        .expect("metadata")
        .modified()
        .expect("mtime")
}

fn file(rig: &Rig, handle: &BlobHandle) -> Vec<u8> {
    fs::read(rig.path(handle.plan.blob_id)).expect("blob file")
}

fn with_bit_flipped(block: &[u8], at: usize) -> Bytes {
    let mut changed = block.to_vec();
    changed[at] ^= 0x01;
    Bytes::from(changed)
}

fn block_offset() -> u64 {
    BlobLayout::new(SIZE as u64)
        .expect("layout")
        .chunk_offset(u64::from(INDEX) * BLOCK as u64 / CHUNK_SIZE)
        .expect("block offset")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_identical_second_write_is_accepted_and_writes_the_stored_bytes_back_unchanged() {
    // Arrange
    let rig = Rig::new().await;
    let source = marked(0xA1, SIZE);
    let handle = written(&rig, &source).await;
    let before = file(&rig, &handle);
    let old = backdate(&rig, &handle);

    // Act
    let again = rig
        .writer
        .write_block(&handle, INDEX, block_of(&source, INDEX as usize))
        .await;

    // Assert
    let token = again.expect("identical bytes are accepted");
    rig.writer.await_durable(token).await.expect("durable");
    assert!(file(&rig, &handle) == before, "the file changed");
    assert!(
        modified(&rig, &handle) > old,
        "the stored block was not written back"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_different_second_write_is_block_conflict_and_the_blob_still_reads_the_first_bytes() {
    // Arrange
    let rig = Rig::new().await;
    let source = marked(0xA2, SIZE);
    let other = marked(0xB2, SIZE);
    let handle = written(&rig, &source).await;
    let before = file(&rig, &handle);
    let old = backdate(&rig, &handle);

    // Act
    let refused = rig
        .writer
        .write_block(&handle, INDEX, block_of(&other, INDEX as usize))
        .await;

    // Assert
    assert!(
        matches!(refused, Err(BlobError::BlockConflict)),
        "{refused:?}"
    );
    assert!(
        file(&rig, &handle) == before,
        "a refused write changed the file"
    );
    assert_eq!(modified(&rig, &handle), old, "a refused write wrote");
    let blob = commit(&rig, rig.account, &handle).await;
    let read = read_all(&rig.reader, blob.blob_id, &blob).await;
    assert!(!read.failed(), "{read:?}");
    assert!(
        read.bytes == *source,
        "the blob does not read the first bytes"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_one_bit_difference_in_the_first_a_middle_or_the_last_byte_is_block_conflict() {
    // Arrange
    let rig = Rig::new().await;
    let source = marked(0xA3, SIZE);
    let handle = written(&rig, &source).await;
    let block = block_of(&source, INDEX as usize);
    let before = file(&rig, &handle);

    // Act
    let mut outcomes = Vec::new();
    for at in [0, BLOCK / 2 + 17, BLOCK - 1] {
        let changed = with_bit_flipped(&block, at);
        outcomes.push((at, rig.writer.write_block(&handle, INDEX, changed).await));
    }

    // Assert
    for (at, outcome) in &outcomes {
        assert!(
            matches!(outcome, Err(BlobError::BlockConflict)),
            "byte {at}: {outcome:?}"
        );
    }
    assert!(
        file(&rig, &handle) == before,
        "a refused write changed the file"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn of_two_concurrent_writers_of_one_index_exactly_one_content_wins_and_every_read_returns_it()
{
    // Arrange
    let rig = Rig::new().await;
    let rounds = 8;
    let mut cases = Vec::new();
    for round in 0..rounds {
        let first = marked(0xC0 + round, SIZE);
        let second = marked(0xD0 + round, SIZE);
        let handle = created(&rig, &rig.writer, false).await;
        for index in [0, 2] {
            write(
                &rig.writer,
                &handle,
                index,
                block_of(&first, index as usize),
            )
            .await;
        }
        cases.push((handle, first, second));
    }

    // Act
    let mut outcomes = Vec::new();
    for (handle, first, second) in &cases {
        let racers = [first, second].map(|source| {
            let (writer, handle) = (Arc::clone(&rig.writer), handle.clone());
            let block = block_of(source, INDEX as usize);
            tokio::spawn(async move {
                match writer.write_block(&handle, INDEX, block).await {
                    Ok(token) => writer.await_durable(token).await,
                    Err(error) => Err(error),
                }
            })
        });
        let mut results = Vec::new();
        for racer in racers {
            results.push(racer.await.expect("writer task"));
        }
        outcomes.push(results);
    }

    // Assert
    for ((handle, first, second), results) in cases.iter().zip(&outcomes) {
        let won: Vec<bool> = results.iter().map(Result::is_ok).collect();
        assert_eq!(won.iter().filter(|ok| **ok).count(), 1, "{results:?}");
        let lost = results.iter().find(|result| result.is_err());
        assert!(
            matches!(lost, Some(Err(BlobError::BlockConflict))),
            "{results:?}"
        );
        let winner = if won[0] { first } else { second };
        let mut expected = first.to_vec();
        let range = BLOCK..2 * BLOCK;
        expected[range.clone()].copy_from_slice(&winner[range]);
        let blob = commit(&rig, rig.account, handle).await;
        for _ in 0..3 {
            let read = read_all(&rig.reader, blob.blob_id, &blob).await;
            assert!(!read.failed(), "{read:?}");
            assert!(read.bytes == expected, "a read returned other bytes");
        }
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_crash_between_write_and_ack_the_same_bytes_are_accepted_through_open_for_write() {
    // Arrange: the block reached the file, the process died before its acknowledgement
    let rig = Rig::new().await;
    let source = marked(0xA5, SIZE);
    let crashed = writer_with(&rig, false);
    let handle = created(&rig, &crashed, false).await;
    crashed
        .write_block(&handle, INDEX, block_of(&source, INDEX as usize))
        .await
        .expect("block is written");
    drop(crashed);
    let before = file(&rig, &handle);
    let old = backdate(&rig, &handle);
    let restarted = writer_with(&rig, false);
    let reopened = restarted
        .open_for_write(
            handle.plan.blob_id,
            &handle.plan.wrapped_key,
            handle.plan.master_key_id,
            SIZE as i64,
        )
        .await
        .expect("open_for_write");

    // Act
    let resent = restarted
        .write_block(&reopened, INDEX, block_of(&source, INDEX as usize))
        .await;

    // Assert
    let token = resent.expect("the re-sent bytes are accepted");
    restarted.await_durable(token).await.expect("durable");
    assert!(
        file(&rig, &handle) == before,
        "the re-send changed the file"
    );
    assert!(
        modified(&rig, &handle) > old,
        "the stored block was not written back"
    );
    drop(restarted);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torn_block_region_is_block_conflict_and_changes_no_byte() {
    // Arrange: garbage over part of one block, and a block whose second half never reached the disk
    let rig = Rig::new().await;
    let tears: [(u64, Vec<u8>); 2] = [
        (300_000, vec![0x5A; 4096]),
        (BLOCK as u64 / 2, vec![0; BLOCK / 2 + REGION_TAGS]),
    ];
    let mut cases = Vec::new();
    for (seed, (at, bytes)) in tears.iter().enumerate() {
        let source = marked(0xE0 + seed as u64, SIZE);
        let handle = written(&rig, &source).await;
        let torn = OpenOptions::new()
            .write(true)
            .open(rig.path(handle.plan.blob_id))
            .expect("blob file");
        torn.write_all_at(bytes, block_offset() + at).expect("tear");
        torn.sync_all().expect("sync");
        let before = file(&rig, &handle);
        cases.push((handle, source, before));
    }

    // Act
    let mut outcomes = Vec::new();
    for (handle, source, _) in &cases {
        let block = block_of(source, INDEX as usize);
        outcomes.push(rig.writer.write_block(handle, INDEX, block).await);
    }

    // Assert
    for ((handle, _, before), outcome) in cases.iter().zip(&outcomes) {
        assert!(
            matches!(outcome, Err(BlobError::BlockConflict)),
            "{outcome:?}"
        );
        assert!(
            file(&rig, handle) == *before,
            "a refused write changed the file"
        );
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_never_written_index_of_a_preallocated_file_is_written_as_a_first_write() {
    // Arrange
    let rig = Rig::new().await;
    let source = marked(0xA7, SIZE);
    let writer = writer_with(&rig, true);
    let handle = created(&rig, &writer, true).await;
    let length = file(&rig, &handle).len();
    for index in [0, 2] {
        write(&writer, &handle, index, block_of(&source, index as usize)).await;
    }

    // Act
    let first = writer
        .write_block(&handle, INDEX, block_of(&source, INDEX as usize))
        .await;

    // Assert
    let token = first.expect("a preallocated region is written");
    writer.await_durable(token).await.expect("durable");
    assert_eq!(
        length as u64,
        BlobLayout::new(SIZE as u64).expect("layout").file_len()
    );
    let blob = commit(&rig, rig.account, &handle).await;
    let read = read_all(&rig.reader, blob.blob_id, &blob).await;
    assert!(!read.failed(), "{read:?}");
    assert!(read.bytes == *source, "the blob does not read its bytes");
    drop(writer);
    rig.finish().await;
}
