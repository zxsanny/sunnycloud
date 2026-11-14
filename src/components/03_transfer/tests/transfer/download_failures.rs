//! Downloads (AZ-3755 AC-5, AC-6; IT-03-14): a corrupt chunk ends the body with an error and never a
//! complete-looking short body, a missing blob file is `ContentUnavailable` and is marked, and a blob reference
//! of the account is served by the same rules. Corruption and deletion are made on disk outside the service.

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use contracts::{
    BlobRef, Disposition, DownloadError, DownloadOptions, DownloadService, DownloadStatus,
    StreamError,
};
use sunnycloud_blobstore::{CHUNK_SIZE, HEADER_LEN, SEALED_CHUNK_LEN, blob_path};
use uuid::Uuid;

use crate::download_support::{drain, ranged, read_all};
use crate::put_support::content;
use crate::start_support::{MIB, Rig};

/// Flips one byte inside the sealed chunk `index` of the blob's file.
fn flip_byte_in_chunk(rig: &Rig, blob: Uuid, index: u64) {
    let path = blob_path(&rig.volume, blob);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("blob file");
    let offset = HEADER_LEN as u64 + index * SEALED_CHUNK_LEN + 100;
    let mut byte = [0u8; 1];
    file.seek(SeekFrom::Start(offset)).expect("seek");
    file.read_exact(&mut byte).expect("read byte");
    byte[0] ^= 0x01;
    file.seek(SeekFrom::Start(offset)).expect("seek back");
    file.write_all(&byte).expect("write byte");
}

#[tokio::test]
async fn open_revision_content_a_corrupt_chunk_ends_the_full_body_with_an_error_and_a_range_before_it_reads()
 {
    // Arrange
    let rig = Rig::new().await;
    let data = content(91, 10 * MIB as usize);
    let committed = rig
        .commit_bytes("ten.bin", "ten", data.clone(), None)
        .await
        .expect("committed");
    let blob = rig.revision_row(committed.revision_id).await.blob_id;
    flip_byte_in_chunk(&rig, blob, 40);
    let clean_end = 40 * CHUNK_SIZE;
    let node = contracts::RevisionOrCurrent::Current(committed.node_id);

    // Act
    let full = rig
        .open(node, DownloadOptions::default())
        .await
        .expect("first chunk verified");
    let (bytes, error) = drain(full.body).await;
    let inner = rig
        .open(node, ranged(1000, clean_end - 1))
        .await
        .expect("range served");

    // Assert
    assert_eq!(full.status, DownloadStatus::Full);
    assert!(matches!(error, Some(StreamError::Corrupt)), "{error:?}");
    assert!(bytes.len() as u64 <= clean_end, "{} bytes", bytes.len());
    assert!(
        bytes == data[..bytes.len()],
        "bytes before the corrupt chunk"
    );
    assert!(
        read_all(inner).await == data[1000..clean_end as usize],
        "range before the corrupt chunk"
    );
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_a_deleted_blob_file_is_content_unavailable_and_marked() {
    // Arrange
    let rig = Rig::new().await;
    let committed = rig
        .commit_bytes("lost.bin", "lost", content(92, 300_000), None)
        .await
        .expect("committed");
    let blob = rig.revision_row(committed.revision_id).await.blob_id;
    fs::remove_file(blob_path(&rig.volume, blob)).expect("file removed");

    // Act
    let result = rig.open_current(committed.node_id).await;

    // Assert
    assert!(matches!(result, Err(DownloadError::ContentUnavailable)));
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM blobs WHERE id = '{blob}' AND unavailable_at IS NOT NULL"
        ))
        .await,
        1
    );
    rig.finish().await;
}

#[tokio::test]
async fn open_blob_content_serves_an_own_blob_with_the_given_type_and_no_name() {
    // Arrange
    let rig = Rig::new().await;
    let data = content(93, 200_000);
    let committed = rig
        .commit_bytes("thumb.bin", "thumb", data.clone(), None)
        .await
        .expect("committed");
    let blob = rig.revision_row(committed.revision_id).await.blob_id;
    let reference = BlobRef {
        blob_id: blob,
        size: 200_000,
        wrapped_key: Vec::new(),
        master_key_id: 0,
    };
    let service: &dyn DownloadService = rig.service.as_ref();

    // Act
    let download = service
        .open_blob_content(
            rig.account,
            reference,
            Some("image/webp".to_owned()),
            ranged(10, 19),
        )
        .await
        .expect("served");

    // Assert
    assert_eq!(download.status, DownloadStatus::Partial);
    assert_eq!(download.etag, blob.to_string());
    assert_eq!(download.disposition, Disposition::Inline);
    assert_eq!(download.media_type.as_deref(), Some("image/webp"));
    assert_eq!(download.suggested_name, "");
    assert_eq!(read_all(download).await, data[10..20]);
    rig.finish().await;
}
