//! AC-4: a damaged file never yields plaintext of a chunk that does not verify. Ranges that avoid the damage are
//! correct; a read that touches it ends with `Corrupt` (at `open` when it is the first chunk, in the stream
//! otherwise) after exactly the verified chunks before it, and the stream then ends.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;
use std::sync::Arc;

use contracts::{BlobError, BlobRef, StreamError};
use sunnycloud_blobstore::{BlobLayout, CHUNK_SIZE, SEALED_CHUNK_LEN};

use crate::support::{Rig, consume, data, next, range};

const CHUNK: usize = CHUNK_SIZE as usize;
const SIZE: usize = 10 * CHUNK + 1000;
const LAST: u64 = 10;

struct Damage {
    name: &'static str,
    first: u64,
    last: u64,
    apply: fn(&Rig, &BlobRef, &BlobRef),
}

fn layout() -> BlobLayout {
    BlobLayout::new(SIZE as u64).expect("layout")
}

fn offset(index: u64) -> u64 {
    layout().chunk_offset(index).expect("chunk offset")
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

fn damages() -> Vec<Damage> {
    vec![
        Damage {
            name: "bit flip in chunk 5",
            first: 5,
            last: 5,
            apply: |rig, blob, _| edit(rig, blob, |file| flip(file, offset(5) + 100)),
        },
        Damage {
            name: "truncated at the end of chunk 4",
            first: 5,
            last: LAST,
            apply: |rig, blob, _| {
                edit(rig, blob, |file| file.set_len(offset(5)).expect("truncate"))
            },
        },
        Damage {
            name: "truncated inside chunk 5",
            first: 5,
            last: LAST,
            apply: |rig, blob, _| {
                edit(rig, blob, |file| {
                    file.set_len(offset(5) + 1000).expect("truncate")
                });
            },
        },
        Damage {
            name: "16 bytes appended",
            first: LAST,
            last: LAST,
            apply: |rig, blob, _| {
                edit(rig, blob, |file| {
                    let end = file.metadata().expect("metadata").len();
                    file.write_all_at(&[0xEE; 16], end).expect("append");
                });
            },
        },
        Damage {
            name: "chunks 2 and 3 swapped",
            first: 2,
            last: 3,
            apply: |rig, blob, _| {
                edit(rig, blob, |file| {
                    let (two, three) = (chunk_of(file, 2), chunk_of(file, 3));
                    file.write_all_at(&three, offset(2)).expect("write");
                    file.write_all_at(&two, offset(3)).expect("write");
                });
            },
        },
        Damage {
            name: "chunk 4 copied from another blob",
            first: 4,
            last: 4,
            apply: |rig, blob, other| {
                let foreign = OpenOptions::new()
                    .read(true)
                    .open(rig.path(other.blob_id))
                    .expect("other file");
                let sealed = chunk_of(&foreign, 4);
                edit(rig, blob, |file| {
                    file.write_all_at(&sealed, offset(4)).expect("write")
                });
            },
        },
        Damage {
            name: "header nonce prefix changed",
            first: 0,
            last: LAST,
            apply: |rig, blob, _| edit(rig, blob, |file| flip(file, 12)),
        },
        Damage {
            name: "final chunk dropped",
            first: LAST,
            last: LAST,
            apply: |rig, blob, _| {
                edit(rig, blob, |file| {
                    file.set_len(offset(LAST)).expect("truncate")
                })
            },
        },
    ]
}

enum Failure {
    Open(BlobError),
    Stream(StreamError),
}

impl Failure {
    fn is_corrupt(&self) -> bool {
        matches!(
            self,
            Failure::Open(BlobError::Corrupt) | Failure::Stream(StreamError::Corrupt)
        )
    }
}

struct Read {
    bytes: Vec<u8>,
    failure: Option<Failure>,
    after_error: usize,
}

async fn read(rig: &Rig, blob: &BlobRef, range: Option<contracts::ByteRange>) -> Read {
    match rig.open(blob, range).await {
        Err(error) => Read {
            bytes: Vec::new(),
            failure: Some(Failure::Open(error)),
            after_error: 0,
        },
        Ok(stream) => {
            let consumed = consume(stream).await;
            Read {
                bytes: consumed.bytes,
                failure: consumed.error.map(Failure::Stream),
                after_error: consumed.after_error,
            }
        }
    }
}

async fn damaged_pair(rig: &Rig, source: &Arc<Vec<u8>>, damage: &Damage) -> BlobRef {
    let blob = rig.store(source).await;
    let other = rig.store(&data(0xDD, SIZE)).await;
    (damage.apply)(rig, &blob, &other);
    blob
}

#[tokio::test(flavor = "multi_thread")]
async fn ranges_avoiding_the_damage_are_correct_and_every_read_touching_it_ends_with_corrupt() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xA4, SIZE);

    for damage in damages() {
        let blob = damaged_pair(&rig, &source, &damage).await;

        for chunk in (0..=LAST).filter(|chunk| *chunk < damage.first || *chunk > damage.last) {
            // Act
            let start = chunk as usize * CHUNK + 100;
            let end = (start + 4095).min(SIZE - 1);
            let outcome = read(&rig, &blob, range(start, end)).await;

            // Assert
            assert!(
                outcome.failure.is_none(),
                "{}: chunk {chunk} avoids the damage",
                damage.name
            );
            assert!(
                outcome.bytes == source[start..=end],
                "{}: chunk {chunk}",
                damage.name
            );
        }
        for chunk in damage.first..=damage.last {
            let start = chunk as usize * CHUNK + 100;
            let end = (start + 4095).min(SIZE - 1);
            let outcome = read(&rig, &blob, range(start, end)).await;

            assert!(
                outcome.failure.as_ref().is_some_and(Failure::is_corrupt),
                "{}: chunk {chunk} is damaged",
                damage.name
            );
            assert!(
                outcome.bytes.is_empty(),
                "{}: bytes of a bad chunk",
                damage.name
            );
        }
        let whole = read(&rig, &blob, None).await;
        assert!(
            whole.failure.as_ref().is_some_and(Failure::is_corrupt),
            "{}: full read",
            damage.name
        );
        assert_eq!(
            whole.bytes.len(),
            damage.first as usize * CHUNK,
            "{}",
            damage.name
        );
        assert!(
            whole.bytes == source[..whole.bytes.len()],
            "{}",
            damage.name
        );
        assert_eq!(whole.after_error, 0, "{}", damage.name);
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_that_runs_into_the_damage_delivers_the_verified_chunks_then_corrupt() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xB4, SIZE);
    let damage = &damages()[0];
    let blob = damaged_pair(&rig, &source, damage).await;
    let (start, end) = (3 * CHUNK + 7, 7 * CHUNK + 7);

    // Act
    let outcome = read(&rig, &blob, range(start, end)).await;

    // Assert
    assert!(outcome.failure.as_ref().is_some_and(Failure::is_corrupt));
    assert_eq!(outcome.bytes.len(), 2 * CHUNK - 7);
    assert!(outcome.bytes == source[start..start + outcome.bytes.len()]);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_ended_with_corrupt_yields_nothing_more() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xC4, SIZE);
    let damage = &damages()[0];
    let blob = damaged_pair(&rig, &source, damage).await;
    let mut stream = rig.open(&blob, None).await.expect("open");
    let mut delivered = 0;
    let error = loop {
        match next(&mut stream).await {
            Some(Ok(bytes)) => delivered += bytes.len(),
            Some(Err(error)) => break error,
            None => panic!("the stream ended without an error"),
        }
    };

    // Act
    let after = [next(&mut stream).await, next(&mut stream).await];

    // Assert
    assert!(matches!(error, StreamError::Corrupt));
    assert_eq!(delivered, 5 * CHUNK);
    assert!(after.iter().all(Option::is_none));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chunk_sealed_as_not_last_is_corrupt_when_the_row_declares_it_the_last() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xD4, SIZE);
    let blob = rig.store(&source).await;
    edit(&rig, &blob, |file| {
        file.set_len(offset(3)).expect("truncate")
    });
    let declared = (3 * CHUNK) as i64;

    // Act
    let stream = contracts::BlobReader::open(
        &rig.reader,
        blob.blob_id,
        &blob.wrapped_key,
        blob.master_key_id,
        declared,
        None,
    )
    .await
    .expect("the first chunk verifies");
    let consumed = consume(stream).await;

    // Assert
    assert!(matches!(consumed.error, Some(StreamError::Corrupt)));
    assert_eq!(consumed.bytes.len(), 2 * CHUNK);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrapped_key_of_another_blob_is_key_unavailable_and_returns_no_byte() {
    // Arrange
    let rig = Rig::new().await;
    let first = rig.store(&data(0xE4, CHUNK)).await;
    let second = rig.store(&data(0xE5, CHUNK)).await;

    // Act
    let opened = contracts::BlobReader::open(
        &rig.reader,
        second.blob_id,
        &first.wrapped_key,
        first.master_key_id,
        second.size,
        None,
    )
    .await;

    // Assert
    assert!(
        matches!(opened, Err(BlobError::KeyUnavailable)),
        "{:?}",
        opened.err()
    );
    assert!(!rig.unavailable(second.blob_id).await);
    rig.finish().await;
}
