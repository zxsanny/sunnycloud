//! `stage_small` (AZ-3737, IT-03-5): sizes from 0 to 2 MiB, the empty file and the idempotent repeat.

use contracts::{Staged, UploadError};
use sc_ids::hash_block;
use uuid::Uuid;

use crate::put_support::content;
use crate::start_support::{MIB, Rig, request, started};

#[tokio::test]
async fn stage_small_zero_one_and_two_mib_are_staged_with_completed_rows() {
    // Arrange
    let rig = Rig::new().await;
    let sizes = [0, 1, 2 * MIB as usize - 1, 2 * MIB as usize];
    let mut total = 0;

    for (seed, size) in sizes.into_iter().enumerate() {
        let key = Uuid::new_v4();
        let data = content(seed as u64 + 1, size);

        // Act
        let result = rig.stage(key, &data).await;

        // Assert
        let size = size as i64;
        total += size;
        assert_eq!(
            result.expect("staged"),
            Staged {
                staged_key: key,
                size
            }
        );
        let row = rig.staged_row(key).await.expect("staged row");
        assert_eq!(row.declared_size, size);
        assert_eq!(row.reserved_bytes, size);
        if size == 0 {
            assert_eq!(row.block_count, 0);
            assert!(row.received.is_empty() && row.block_hashes.is_empty());
        } else {
            assert_eq!(row.block_count, 1, "size {size}");
            assert_eq!(row.received, vec![1]);
            assert_eq!(row.block_hashes, hash_block(&data).as_bytes().to_vec());
        }
        assert_eq!(
            rig.read_blob(row.blob, data.len()).await,
            data,
            "size {size}"
        );
    }
    assert_eq!(rig.sessions().await, 4);
    assert_eq!(rig.uploading_blobs().await, 4);
    assert_eq!(rig.files(), 4);
    assert_eq!(rig.reserved().await, total);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_an_empty_file_may_carry_the_hash_of_no_bytes() {
    // Arrange
    let rig = Rig::new().await;
    let key = Uuid::new_v4();

    // Act
    let (result, _) = rig.stage_raw(key, Some(hash_block(&[])), &[], 0).await;
    let (repeat, _) = rig.stage_raw(key, None, &[], 0).await;

    // Assert
    assert_eq!(result.expect("staged").size, 0);
    assert_eq!(repeat.expect("repeat staged").size, 0);
    assert_eq!(rig.staged_row(key).await.expect("row").block_count, 0);
    assert_eq!(rig.files(), 1);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_above_two_mib_is_a_length_mismatch_and_nothing_is_stored() {
    // Arrange
    let rig = Rig::new().await;
    let data = content(7, 2 * MIB as usize + 1);

    // Act
    let (result, taken) = rig
        .stage_raw(
            Uuid::new_v4(),
            Some(hash_block(&data)),
            &data,
            data.len() as u64,
        )
        .await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::LengthMismatch)),
        "{result:?}"
    );
    assert_eq!(taken, 0);
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.uploading_blobs().await, 0);
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_a_body_shorter_or_longer_than_its_length_is_a_length_mismatch() {
    // Arrange
    let rig = Rig::new().await;
    let data = content(8, 1000);
    let hash = Some(hash_block(&data));

    // Act
    let (short, _) = rig.stage_raw(Uuid::new_v4(), hash, &data, 1001).await;
    let (long, _) = rig.stage_raw(Uuid::new_v4(), hash, &data, 999).await;

    // Assert
    assert!(
        matches!(short, Err(UploadError::LengthMismatch)),
        "{short:?}"
    );
    assert!(matches!(long, Err(UploadError::LengthMismatch)), "{long:?}");
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_a_repeat_is_staged_without_a_write_and_another_hash_or_size_mismatches() {
    // Arrange
    let rig = Rig::new().await;
    let key = Uuid::new_v4();
    let data = content(9, 4096);
    let other = content(10, 4096);
    rig.stage(key, &data).await.expect("first stage");
    let row = rig.staged_row(key).await;

    // Act
    let (repeat, repeat_taken) = rig
        .stage_raw(key, Some(hash_block(&data)), &data, 4096)
        .await;
    let (other_hash, other_taken) = rig
        .stage_raw(key, Some(hash_block(&other)), &other, 4096)
        .await;
    let (other_size, _) = rig
        .stage_raw(key, Some(hash_block(&data[..100])), &data[..100], 100)
        .await;

    // Assert
    assert_eq!(
        repeat.expect("repeat"),
        Staged {
            staged_key: key,
            size: 4096
        }
    );
    assert_eq!(repeat_taken, 0);
    assert!(
        matches!(other_hash, Err(UploadError::ClientKeyMismatch)),
        "{other_hash:?}"
    );
    assert_eq!(other_taken, 0);
    assert!(
        matches!(other_size, Err(UploadError::ClientKeyMismatch)),
        "{other_size:?}"
    );
    assert_eq!(rig.staged_row(key).await, row);
    assert_eq!(rig.sessions().await, 1);
    assert_eq!(rig.files(), 1);
    assert_eq!(rig.reserved().await, 4096);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_a_key_held_by_a_large_session_is_a_mismatch() {
    // Arrange
    let rig = Rig::new().await;
    let key = Uuid::new_v4();
    started(
        rig.start(request(rig.root, "big.bin", 10 * MIB, &key.to_string()))
            .await,
    );
    let data = content(11, 100);

    // Act
    let (result, taken) = rig
        .stage_raw(key, Some(hash_block(&data)), &data, 100)
        .await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::ClientKeyMismatch)),
        "{result:?}"
    );
    assert_eq!(taken, 0);
    assert_eq!(rig.sessions().await, 1);
    assert_eq!(rig.reserved().await, 10 * MIB);
    rig.finish().await;
}
