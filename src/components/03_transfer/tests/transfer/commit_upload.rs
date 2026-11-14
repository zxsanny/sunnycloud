//! `commit_upload` (AZ-3738 AC-1 to AC-3; IT-03-6): the file hash is the roll-up of the block hashes and only the
//! head is read, the media type comes from the bytes, and the blocklist is checked before the commit
//! transaction. Real 02 `publish`, 04 reader and registry, 05 stamp and 07a ledger.

use contracts::{CommitServiceError, UploadError};
use sc_ids::Hash32;
use uuid::Uuid;

use crate::commit_support::{commit_request, file_hash};
use crate::put_support::content;
use crate::start_support::{MIB, Rig, TTL, now};

/// 64 KiB: the most `read_head` returns (`commit_service.md`).
const HEAD_MAX: u32 = 64 * 1024;

const JPEG: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00";
const HTML: &[u8] = b"<!DOCTYPE html><html><body><script>alert(1)</script></body></html>";

/// `head` followed by deterministic filler up to `len` bytes.
fn file_with_head(head: &[u8], len: usize, seed: u64) -> Vec<u8> {
    let mut data = head.to_vec();
    data.extend(content(seed, len - head.len()));
    data
}

#[tokio::test]
async fn commit_upload_hashes_from_the_block_hashes_and_reads_only_the_head() {
    // Arrange
    let rig = Rig::new().await;
    let data = content(81, 5 * MIB as usize + 1);
    let upload = rig.upload_bytes("big.bin", "ac1", data.clone()).await;
    rig.send_all(&upload).await;
    rig.reader.reset();

    // Act
    let committed = rig
        .commit(commit_request(&upload, rig.root, "big.bin", None))
        .await
        .expect("committed");
    let reads = rig.reader.counts();

    // Assert
    let revision = rig.revision_row(committed.revision_id).await;
    assert_eq!(revision.content_hash, file_hash(&data).as_bytes().to_vec());
    assert_eq!(revision.size, i64::try_from(data.len()).expect("size"));
    let (opens, heads, largest) = reads;
    assert_eq!((opens, heads), (0, 1), "one head read and no full read");
    assert!(largest <= HEAD_MAX, "head of {largest} bytes");
    assert!(
        rig.read_back(&upload).await == data,
        "committed file intact"
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_takes_the_media_type_from_the_bytes_only() {
    // Arrange
    let rig = Rig::new().await;
    let jpeg = rig
        .upload_bytes(
            "photo.png",
            "jpeg",
            file_with_head(JPEG, 3 * MIB as usize, 82),
        )
        .await;
    let html = rig
        .upload_bytes("x.jpg", "html", file_with_head(HTML, 4096, 83))
        .await;
    let random = rig
        .upload_bytes("data.bin", "random", content(84, 9000))
        .await;
    for upload in [&jpeg, &html, &random] {
        rig.send_all(upload).await;
    }
    let mut jpeg_request = commit_request(&jpeg, rig.root, "photo.png", None);
    jpeg_request.client_meta = Some(r#"{"content_type":"text/html"}"#.to_owned());

    // Act
    let jpeg_commit = rig.commit(jpeg_request).await.expect("jpeg committed");
    let html_commit = rig
        .commit(commit_request(&html, rig.root, "x.jpg", None))
        .await
        .expect("html committed");
    let random_commit = rig
        .commit(commit_request(&random, rig.root, "data.bin", None))
        .await
        .expect("random committed");

    // Assert
    let types = [
        rig.revision_row(jpeg_commit.revision_id).await.media_type,
        rig.revision_row(html_commit.revision_id).await.media_type,
        rig.revision_row(random_commit.revision_id).await.media_type,
    ];
    assert_eq!(
        types,
        [
            Some("image/jpeg".to_owned()),
            Some("text/html".to_owned()),
            None
        ]
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_with_a_block_missing_needs_that_block() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(10 * MIB as usize, 85).await;
    for index in [0, 1, 3, 4] {
        rig.put(&upload, index).await.expect("block acknowledged");
    }

    // Act
    let result = rig
        .commit(commit_request(&upload, rig.root, "gap.bin", None))
        .await;

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::NeedBlocks(ref need)) if need == &[2]),
        "{result:?}"
    );
    assert_eq!(rig.revisions().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_with_a_wrong_hash_or_count_is_a_blocklist_mismatch() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(10 * MIB as usize, 86).await;
    rig.send_all(&upload).await;
    let mut wrong_hash = commit_request(&upload, rig.root, "five.bin", None);
    wrong_hash.blocklist[1] = Hash32::from_bytes([0xee; 32]);
    let mut wrong_count = commit_request(&upload, rig.root, "five.bin", None);
    wrong_count.blocklist.truncate(3);

    // Act
    let results = [rig.commit(wrong_hash).await, rig.commit(wrong_count).await];

    // Assert
    for result in results {
        assert!(
            matches!(result, Err(CommitServiceError::BlocklistMismatch)),
            "{result:?}"
        );
    }
    assert_eq!(rig.revisions().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_of_an_unknown_upload_is_not_found() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(MIB as usize, 87).await;
    let mut request = commit_request(&upload, rig.root, "unknown.bin", None);
    request.upload_id = Uuid::new_v4();

    // Act
    let result = rig.commit(request).await;

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::UploadNotFound)),
        "{result:?}"
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_closes_the_session_into_its_committed_key_record() {
    // Arrange
    let rig = Rig::new().await;
    let size = 10 * MIB;
    let upload = rig.upload(size as usize, 88).await;
    rig.send_all(&upload).await;
    let committed_at = now(&rig.clock);

    // Act
    let committed = rig
        .commit(commit_request(&upload, rig.root, "closed.bin", None))
        .await
        .expect("committed");
    let after = rig.service.get_upload(rig.account, upload.id).await;

    // Assert
    let record = rig.session_record(upload.id).await.expect("record kept");
    assert_eq!(record.committed_revision_id, Some(committed.revision_id));
    assert_eq!(record.reserved_bytes, 0);
    assert_eq!(
        record.expires_at,
        committed_at.checked_add(TTL).expect("expiry")
    );
    assert!(
        matches!(after, Err(UploadError::UploadNotFound)),
        "the open session is gone: {after:?}"
    );
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(rig.used().await, size);
    assert_eq!(
        rig.blob_state(upload.blob).await.as_deref(),
        Some("committed")
    );
    rig.finish().await;
}
