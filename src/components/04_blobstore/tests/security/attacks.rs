//! ST-04-3 (AC-6): active attacks on the ciphertext. The 8 damage cases of IT-04-4, a rolled-back file, a whole
//! file moved to another blob id and one blob's wrapped key used for another are each applied to the files of
//! real blobs, then the blob is opened and consumed. No read completes, and no byte of a chunk at or after the
//! first damaged one is returned: only the verified chunks before it, byte-equal to the source.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;

use contracts::{BlobError, BlobRef, BlobWriter, StreamError};
use sunnycloud_blobstore::{BlobLayout, CHUNK_SIZE, SEALED_CHUNK_LEN};

use crate::kit::{BLOCK, Read, block_of, commit, marked, read_all, store};
use crate::support::Rig;

const CHUNK: usize = CHUNK_SIZE as usize;
const SIZE: usize = 10 * CHUNK + 1000;
const LAST: u64 = 10;
/// Byte 12 of the header is the first byte of the nonce prefix.
const PREFIX_BYTE: u64 = 12;

fn offset(index: u64) -> u64 {
    BlobLayout::new(SIZE as u64)
        .expect("layout")
        .chunk_offset(index)
        .expect("chunk offset")
}

fn edit(rig: &Rig, blob: &BlobRef, change: impl FnOnce(&fs::File)) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(rig.path(blob.blob_id))
        .expect("blob file");
    change(&file);
    file.sync_all().expect("sync");
}

fn flip(file: &fs::File, at: u64) {
    let mut byte = [0];
    file.read_exact_at(&mut byte, at).expect("read byte");
    byte[0] ^= 0x01;
    file.write_all_at(&byte, at).expect("write byte");
}

fn chunk_of(file: &fs::File, index: u64) -> Vec<u8> {
    let mut sealed = vec![0; SEALED_CHUNK_LEN as usize];
    file.read_exact_at(&mut sealed, offset(index))
        .expect("read chunk");
    sealed
}

type Apply = fn(&Rig, &BlobRef, &BlobRef);

/// The IT-04-4 cases: name, first damaged chunk, damage.
fn damages() -> [(&'static str, u64, Apply); 8] {
    [
        ("(a) bit flip in chunk 5", 5, |rig, blob, _| {
            edit(rig, blob, |file| flip(file, offset(5) + 100));
        }),
        ("(b) truncated at a chunk boundary", 5, |rig, blob, _| {
            edit(rig, blob, |file| file.set_len(offset(5)).expect("truncate"));
        }),
        ("(c) truncated inside a chunk", 5, |rig, blob, _| {
            edit(rig, blob, |file| {
                file.set_len(offset(5) + 1000).expect("truncate");
            });
        }),
        ("(d) 16 bytes appended", LAST, |rig, blob, _| {
            edit(rig, blob, |file| {
                let end = file.metadata().expect("metadata").len();
                file.write_all_at(&[0xEE; 16], end).expect("append");
            });
        }),
        ("(e) chunks 2 and 3 swapped", 2, |rig, blob, _| {
            edit(rig, blob, |file| {
                let (two, three) = (chunk_of(file, 2), chunk_of(file, 3));
                file.write_all_at(&three, offset(2)).expect("write");
                file.write_all_at(&two, offset(3)).expect("write");
            });
        }),
        (
            "(f) chunk 4 copied from another blob",
            4,
            |rig, blob, other| {
                let foreign = fs::File::open(rig.path(other.blob_id)).expect("other file");
                let sealed = chunk_of(&foreign, 4);
                edit(rig, blob, |file| {
                    file.write_all_at(&sealed, offset(4)).expect("write");
                });
            },
        ),
        ("(g) header nonce prefix changed", 0, |rig, blob, _| {
            edit(rig, blob, |file| flip(file, PREFIX_BYTE));
        }),
        ("(h) final-flag chunk dropped", LAST, |rig, blob, _| {
            edit(rig, blob, |file| {
                file.set_len(offset(LAST)).expect("truncate")
            });
        }),
    ]
}

/// No complete read, and only verified plaintext of the chunks before `first` was returned.
fn assert_refused(read: &Read, source: &[u8], first: usize, what: &str) {
    assert!(read.failed(), "{what}: the read completed");
    assert!(
        read.bytes.len() <= first * CHUNK,
        "{what}: {} bytes returned, damage starts at byte {}",
        read.bytes.len(),
        first * CHUNK
    );
    assert_eq!(
        read.bytes[..],
        source[..read.bytes.len()],
        "{what}: bytes differ"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_damage_case_ends_the_read_with_corrupt_before_any_damaged_byte() {
    // Arrange
    let rig = Rig::new().await;
    let other = store(&rig, rig.account, &marked(0xDD, SIZE)).await;
    let mut cases = Vec::new();
    for (index, (name, first, apply)) in damages().into_iter().enumerate() {
        let source = marked(index as u64 + 10, SIZE);
        let blob = store(&rig, rig.account, &source).await;
        apply(&rig, &blob, &other);
        cases.push((name, first, source, blob));
    }

    // Act
    let mut reads = Vec::new();
    for (_, _, _, blob) in &cases {
        reads.push(read_all(&rig.reader, blob.blob_id, blob).await);
    }

    // Assert
    for ((name, first, source, _), read) in cases.iter().zip(&reads) {
        assert_refused(read, source, *first as usize, name);
        assert!(
            matches!(read.open, Some(BlobError::Corrupt))
                || matches!(read.stream, Some(StreamError::Corrupt)),
            "{name}: {read:?}"
        );
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_rolled_back_to_an_earlier_copy_is_corrupt_past_the_rolled_back_part() {
    // Arrange: a copy of the file taken while only block 0 was written, restored after the commit
    let rig = Rig::new().await;
    let source = marked(0x70, 2 * BLOCK + 1000);
    let plan = rig
        .writer
        .plan(rig.account, source.len() as i64)
        .expect("plan");
    let handle = rig
        .writer
        .create_file(plan, source.len() as i64, false)
        .await
        .expect("create_file");
    let mut snapshot = Vec::new();
    for index in 0..3 {
        let token = rig
            .writer
            .write_block(&handle, index as u32, block_of(&source, index))
            .await
            .expect("block");
        rig.writer.await_durable(token).await.expect("durable");
        if index == 0 {
            snapshot = fs::read(rig.path(handle.plan.blob_id)).expect("copy");
        }
    }
    let blob = commit(&rig, rig.account, &handle).await;
    let complete = fs::read(rig.path(blob.blob_id)).expect("file");
    fs::write(rig.path(blob.blob_id), &snapshot).expect("restore the earlier copy");

    // Act
    let read = read_all(&rig.reader, blob.blob_id, &blob).await;

    // Assert
    assert_ne!(snapshot, complete, "the copy is an earlier state");
    assert_refused(&read, &source, BLOCK / CHUNK, "rolled-back file");
    assert!(
        matches!(read.stream, Some(StreamError::Corrupt)),
        "{read:?}"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_whole_file_moved_to_another_blob_id_is_corrupt_and_returns_no_byte() {
    // Arrange
    let rig = Rig::new().await;
    let moved = store(&rig, rig.account, &marked(0x81, SIZE)).await;
    let target_source = marked(0x82, SIZE);
    let target = store(&rig, rig.account, &target_source).await;
    fs::copy(rig.path(moved.blob_id), rig.path(target.blob_id)).expect("move the file");

    // Act
    let read = read_all(&rig.reader, target.blob_id, &target).await;

    // Assert
    assert_refused(&read, &target_source, 0, "moved file");
    assert!(matches!(read.open, Some(BlobError::Corrupt)), "{read:?}");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wrapped_key_of_one_blob_used_for_another_is_key_unavailable_and_returns_no_byte() {
    // Arrange
    let rig = Rig::new().await;
    let first = store(&rig, rig.account, &marked(0x91, SIZE)).await;
    let second_source = marked(0x92, SIZE);
    let second = store(&rig, rig.account, &second_source).await;
    let spliced = BlobRef {
        wrapped_key: first.wrapped_key.clone(),
        ..second.clone()
    };

    // Act
    let read = read_all(&rig.reader, second.blob_id, &spliced).await;

    // Assert
    assert_refused(&read, &second_source, 0, "foreign wrapped key");
    assert!(
        matches!(read.open, Some(BlobError::KeyUnavailable)),
        "{read:?}"
    );
    rig.finish().await;
}
