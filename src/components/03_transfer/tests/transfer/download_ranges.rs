//! Downloads (AZ-3755 AC-1; IT-03-13): one range honoured, the strong ETag is the blob id, `If-Range` and
//! `If-None-Match`, and `download_bytes_total`. Real 02 reader, 04 reader and catalog.
//!
//! 11 reads `Range` (`content/request.rs`): several ranges, overlapping ranges and a suffix range `bytes=-n` are
//! ignored there and reach 03 as no range, so 03 serves them `Full` like a request without one.

use contracts::{
    ByteRange, Disposition, DownloadError, DownloadOptions, DownloadStatus, RevisionOrCurrent,
};
use sc_observability::{BuildInfo, Metrics};

use crate::download_support::{drain, from, quoted, ranged, read_all};
use crate::put_support::content;
use crate::start_support::{MIB, Rig};

const SIZE: usize = 5 * MIB as usize;

/// A committed 5 MiB file: its node, its current revision, its blob and its bytes.
async fn five_mib(rig: &Rig) -> (RevisionOrCurrent, String, Vec<u8>) {
    let data = content(55, SIZE);
    let committed = rig
        .commit_bytes("five.bin", "five", data.clone(), None)
        .await
        .expect("committed");
    let blob = rig.revision_row(committed.revision_id).await.blob_id;
    (
        RevisionOrCurrent::Current(committed.node_id),
        blob.to_string(),
        data,
    )
}

#[tokio::test]
async fn open_revision_content_without_a_range_is_full_with_the_blob_id_as_etag() {
    // Arrange
    let rig = Rig::new().await;
    let (node, blob, data) = five_mib(&rig).await;

    // Act
    let download = rig
        .open(node, DownloadOptions::default())
        .await
        .expect("served");

    // Assert
    assert_eq!(download.status, DownloadStatus::Full);
    assert_eq!(download.total_size, SIZE as i64);
    assert_eq!(download.range, None);
    assert_eq!(download.etag, blob);
    assert_eq!(download.suggested_name, "five.bin");
    assert_eq!(download.disposition, Disposition::Attachment);
    assert!(read_all(download).await == data, "whole content");
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_one_range_is_partial_with_exactly_its_bytes() {
    // Arrange
    let rig = Rig::new().await;
    let (node, _, data) = five_mib(&rig).await;
    let last = SIZE as u64 - 1;
    let cases = [
        (ranged(0, 0), ByteRange { start: 0, end: 0 }),
        (
            from(4_194_304),
            ByteRange {
                start: 4_194_304,
                end: last,
            },
        ),
        (
            from(SIZE as u64 - 4096),
            ByteRange {
                start: SIZE as u64 - 4096,
                end: last,
            },
        ),
    ];

    for (options, expected) in cases {
        // Act
        let download = rig.open(node, options).await.expect("served");

        // Assert
        assert_eq!(download.status, DownloadStatus::Partial);
        assert_eq!(download.range, Some(expected));
        assert_eq!(download.total_size, SIZE as i64);
        let bytes = read_all(download).await;
        assert!(
            bytes == data[expected.start as usize..=expected.end as usize],
            "bytes of {expected:?}"
        );
    }
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_a_range_past_the_end_is_not_satisfiable() {
    // Arrange
    let rig = Rig::new().await;
    let (node, _, _) = five_mib(&rig).await;

    // Act
    let result = rig.open(node, from(9_999_999)).await;

    // Assert
    assert!(matches!(
        result,
        Err(DownloadError::RangeNotSatisfiable { total_size }) if total_size == SIZE as i64
    ));
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_a_stale_if_range_is_full_and_a_current_if_none_match_is_not_modified()
 {
    // Arrange
    let rig = Rig::new().await;
    let (node, blob, data) = five_mib(&rig).await;
    let stale = DownloadOptions {
        if_range: Some(quoted("0193a2b4-0000-7000-8000-00000000dead")),
        ..ranged(0, 0)
    };
    let fresh = DownloadOptions {
        if_range: Some(quoted(&blob)),
        ..ranged(0, 0)
    };
    let unchanged = DownloadOptions {
        if_none_match: Some(quoted(&blob)),
        ..DownloadOptions::default()
    };

    // Act
    let stale = rig.open(node, stale).await.expect("stale served");
    let fresh = rig.open(node, fresh).await.expect("fresh served");
    let unchanged = rig.open(node, unchanged).await.expect("not modified");

    // Assert
    assert_eq!(stale.status, DownloadStatus::Full);
    assert!(read_all(stale).await == data, "whole content");
    assert_eq!(fresh.status, DownloadStatus::Partial);
    assert_eq!(read_all(fresh).await, data[..1]);
    assert_eq!(unchanged.status, DownloadStatus::NotModified);
    assert_eq!(unchanged.etag, blob);
    let (body, error) = drain(unchanged.body).await;
    assert!(body.is_empty() && error.is_none(), "no body");
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_counts_the_bytes_handed_out() {
    // Arrange
    let rig = Rig::new().await;
    let (node, _, _) = five_mib(&rig).await;
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let service = rig
        .instance()
        .with_metrics(&metrics)
        .expect("upload series");

    // Act
    let download = service
        .open_revision_content(rig.account, node, ranged(10, 4105))
        .await
        .expect("served");
    let bytes = read_all(download).await;
    let rendered = metrics.render().await.expect("render");

    // Assert
    assert_eq!(bytes.len(), 4096);
    assert!(rendered.contains("download_bytes_total 4096"), "{rendered}");
    drop(service);
    rig.finish().await;
}
