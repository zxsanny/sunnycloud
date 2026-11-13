//! `put_block` AC-1 to AC-3 (IT-03-2): correct blocks are acknowledged after durable, a bad hash, length or
//! index never reaches the file, and repeats answer by their hash. Real PostgreSQL, the real 04 writer on a
//! temporary volume.

use std::time::Duration;

use contracts::{BlockAck, UploadError};
use sc_ids::{BLOCK_SIZE, hash_block};
use uuid::Uuid;

use crate::put_support::{NINE_MIB, content};
use crate::start_support::{MIB, Rig, TTL};

const TWO_MIB: u64 = BLOCK_SIZE as u64;

#[tokio::test]
async fn put_block_nine_mib_of_correct_blocks_are_acked_and_the_tail_is_one_mib() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(NINE_MIB, 1).await;
    let tail = upload.block(4).to_vec();

    // Act
    let mut answers = Vec::new();
    for index in 0..4 {
        answers.push(
            rig.put(&upload, index)
                .await
                .map_err(|error| error.to_string()),
        );
    }
    let (tail_as_full, _) = rig
        .put_raw(upload.id, 4, upload.hash(4), &tail, TWO_MIB)
        .await;
    let (tail_exact, _) = rig
        .put_raw(upload.id, 4, upload.hash(4), &tail, MIB as u64)
        .await;
    let stored = rig.read_back(&upload).await;

    // Assert
    assert_eq!(answers, vec![Ok(BlockAck); 4]);
    assert!(
        matches!(tail_as_full, Err(UploadError::LengthMismatch)),
        "{tail_as_full:?}"
    );
    assert!(matches!(tail_exact, Ok(BlockAck)), "{tail_exact:?}");
    assert_eq!(tail.len(), MIB as usize);
    assert!(stored == upload.data, "the file holds the sent bytes");
    rig.finish().await;
}

#[tokio::test]
async fn put_block_a_flipped_bit_is_a_hash_mismatch_and_nothing_is_written() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(NINE_MIB, 2).await;
    let mut flipped = upload.block(0).to_vec();
    flipped[12_345] ^= 0b0000_0100;
    let file_before = rig.file_bytes(upload.blob);

    // Act
    let (result, _) = rig
        .put_raw(upload.id, 0, upload.hash(0), &flipped, TWO_MIB)
        .await;
    let file_after = rig.file_bytes(upload.blob);
    let writes_after_refusal = rig.writer.writes();
    let resend = rig.put(&upload, 0).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::HashMismatch)),
        "{result:?}"
    );
    assert!(file_after == file_before, "file bytes unchanged");
    assert_eq!(writes_after_refusal, 0);
    assert!(matches!(resend, Ok(BlockAck)), "{resend:?}");
    assert_eq!(
        rig.writer.writes(),
        1,
        "the bit was unset: the resend is written"
    );
    rig.finish().await;
}

#[tokio::test]
async fn put_block_wrong_lengths_are_refused_and_a_declared_mismatch_reads_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(NINE_MIB, 3).await;
    let block = upload.block(1).to_vec();
    let hash = upload.hash(1);
    let mut long = block.clone();
    long.push(7);
    let short = &block[..block.len() - 1];
    let file_before = rig.file_bytes(upload.blob);

    // Act
    let cases = [
        rig.put_raw(upload.id, 1, hash, short, TWO_MIB - 1).await,
        rig.put_raw(upload.id, 1, hash, &long, TWO_MIB + 1).await,
        // A chunked body without a length reaches the port with no declared length (0).
        rig.put_raw(upload.id, 1, hash, &block, 0).await,
    ];
    let lying = [
        rig.put_raw(upload.id, 1, hash, short, TWO_MIB).await,
        rig.put_raw(upload.id, 1, hash, &long, TWO_MIB).await,
    ];

    // Assert
    for (result, read) in &cases {
        assert!(
            matches!(result, Err(UploadError::LengthMismatch)),
            "{result:?}"
        );
        assert_eq!(*read, 0, "refused before the body");
    }
    for (result, _) in &lying {
        assert!(
            matches!(result, Err(UploadError::LengthMismatch)),
            "{result:?}"
        );
    }
    assert!(rig.file_bytes(upload.blob) == file_before);
    assert_eq!(rig.writer.writes(), 0);
    rig.finish().await;
}

#[tokio::test]
async fn put_block_an_index_past_the_last_block_is_out_of_range() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(NINE_MIB, 4).await;
    let block = upload.block(0).to_vec();
    let hash = upload.hash(0);

    // Act: index 5 of 5 blocks, and -1 as it arrives in the port's u32.
    let past = rig.put_raw(upload.id, 5, hash, &block, TWO_MIB).await;
    let negative = rig
        .put_raw(upload.id, u32::MAX, hash, &block, TWO_MIB)
        .await;

    // Assert
    for (result, read) in [past, negative] {
        assert!(
            matches!(result, Err(UploadError::IndexOutOfRange)),
            "{result:?}"
        );
        assert_eq!(read, 0);
    }
    assert_eq!(rig.writer.writes(), 0);
    rig.finish().await;
}

#[tokio::test]
async fn put_block_a_repeat_is_acked_without_a_write_and_other_bytes_conflict() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(NINE_MIB, 5).await;
    rig.put(&upload, 0).await.expect("block 0 acked");
    let file_before = rig.file_bytes(upload.blob);
    let other = content(99, BLOCK_SIZE);

    // Act
    let repeat = rig.put(&upload, 0).await;
    let writes_after_repeat = rig.writer.writes();
    let (conflict, read) = rig
        .put_raw(upload.id, 0, hash_block(&other), &other, TWO_MIB)
        .await;

    // Assert
    assert!(matches!(repeat, Ok(BlockAck)), "{repeat:?}");
    assert_eq!(writes_after_repeat, 1, "no second write");
    assert!(
        matches!(conflict, Err(UploadError::BlockConflict)),
        "{conflict:?}"
    );
    assert_eq!(read, 0);
    assert!(rig.file_bytes(upload.blob) == file_before, "file unchanged");
    assert_eq!(rig.writer.writes(), 1);
    rig.finish().await;
}

#[tokio::test]
async fn put_block_an_unknown_foreign_or_expired_session_is_not_found() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(NINE_MIB, 6).await;
    let block = upload.block(0).to_vec();
    let hash = upload.hash(0);
    let (stream, _) = crate::put_support::body(&block, BLOCK_SIZE);

    // Act
    let (unknown, _) = rig.put_raw(Uuid::new_v4(), 0, hash, &block, TWO_MIB).await;
    let foreign = rig
        .service
        .put_block(Uuid::new_v4(), upload.id, 0, hash, stream, TWO_MIB)
        .await;
    rig.clock.tick(TTL + Duration::from_secs(1));
    let (expired, read) = rig.put_raw(upload.id, 0, hash, &block, TWO_MIB).await;

    // Assert
    for result in [unknown, foreign, expired] {
        assert!(
            matches!(result, Err(UploadError::UploadNotFound)),
            "{result:?}"
        );
    }
    assert_eq!(read, 0);
    assert_eq!(rig.writer.writes(), 0);
    rig.finish().await;
}
