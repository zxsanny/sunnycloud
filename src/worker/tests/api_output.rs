//! AZ-3869 AC-5: the output request carries the passed headers and the file as its body.

mod api_support;

use api_support::{MIB, Peer, Reply, Scratch, client, pattern};
use media_shared::types::{DerivedKind, OutputMeta};
use sunnycloud_worker::ApiError;
use uuid::Uuid;

const JOB_ID: Uuid = Uuid::from_u128(0x5eed);

#[tokio::test]
async fn a_webp_preview_carries_type_and_dimensions_and_its_bytes() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(204)).await;
    let scratch = Scratch::new(JOB_ID);
    let bytes = pattern(300_000);
    scratch.write_output("webp", &bytes);
    let output = scratch.dir.open_output("webp", 20 * MIB).expect("output");
    let meta = OutputMeta {
        media_type: "image/webp".into(),
        width: Some(1440),
        height: Some(960),
        duration_ms: None,
    };

    // Act
    let result = client(&peer.base_url())
        .post_output(JOB_ID, DerivedKind::Preview, &meta, output)
        .await;

    // Assert
    result.expect("accepted");
    let seen = peer.requests();
    assert_eq!(seen[0].method, "PUT");
    assert_eq!(
        seen[0].path,
        format!("/api/v1/worker-jobs/{JOB_ID}/outputs/preview")
    );
    assert_eq!(seen[0].header("content-type"), Some("image/webp"));
    assert_eq!(seen[0].header("x-width"), Some("1440"));
    assert_eq!(seen[0].header("x-height"), Some("960"));
    assert_eq!(seen[0].header("x-duration-ms"), None);
    assert_eq!(seen[0].header("content-length"), Some("300000"));
    assert_eq!(seen[0].body, bytes);
}

#[tokio::test]
async fn a_transcode_carries_width_height_and_duration_and_streams_a_multi_chunk_file() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(204)).await;
    let scratch = Scratch::new(JOB_ID);
    let bytes = pattern(3 * MIB as usize + 17);
    scratch.write_output("mp4", &bytes);
    let output = scratch.dir.open_output("mp4", 10 * MIB).expect("output");
    let meta = OutputMeta {
        media_type: "video/mp4".into(),
        width: Some(1920),
        height: Some(1080),
        duration_ms: Some(95_250),
    };

    // Act
    let result = client(&peer.base_url())
        .post_output(JOB_ID, DerivedKind::Transcode, &meta, output)
        .await;

    // Assert
    result.expect("accepted");
    let seen = peer.requests();
    assert_eq!(
        seen[0].path,
        format!("/api/v1/worker-jobs/{JOB_ID}/outputs/transcode")
    );
    assert_eq!(seen[0].header("content-type"), Some("video/mp4"));
    assert_eq!(seen[0].header("x-width"), Some("1920"));
    assert_eq!(seen[0].header("x-height"), Some("1080"));
    assert_eq!(seen[0].header("x-duration-ms"), Some("95250"));
    assert_eq!(seen[0].body, bytes);
}

#[tokio::test]
async fn a_thumb_without_dimensions_sends_no_dimension_headers_and_an_empty_file_is_an_empty_body()
{
    // Arrange
    let peer = Peer::start(|_| Reply::status(200)).await;
    let scratch = Scratch::new(JOB_ID);
    scratch.write_output("webp", b"");
    let output = scratch.dir.open_output("webp", MIB).expect("output");
    let meta = OutputMeta {
        media_type: "image/webp".into(),
        width: None,
        height: None,
        duration_ms: None,
    };

    // Act
    let result = client(&peer.base_url())
        .post_output(JOB_ID, DerivedKind::Thumb, &meta, output)
        .await;

    // Assert
    result.expect("already present counts as success");
    let seen = peer.requests();
    assert_eq!(
        seen[0].path,
        format!("/api/v1/worker-jobs/{JOB_ID}/outputs/thumb")
    );
    assert_eq!(seen[0].header("x-width"), None);
    assert_eq!(seen[0].header("x-height"), None);
    assert!(seen[0].body.is_empty());
}

#[tokio::test]
async fn a_media_type_that_is_not_a_header_value_is_refused_without_a_request() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(204)).await;
    let scratch = Scratch::new(JOB_ID);
    scratch.write_output("webp", b"bytes");
    let output = scratch.dir.open_output("webp", MIB).expect("output");
    let meta = OutputMeta {
        media_type: "image/webp\r\nX-Evil: 1".into(),
        width: None,
        height: None,
        duration_ms: None,
    };

    // Act
    let result = client(&peer.base_url())
        .post_output(JOB_ID, DerivedKind::Thumb, &meta, output)
        .await;

    // Assert
    assert!(matches!(result, Err(ApiError::Protocol(_))), "{result:?}");
    assert!(peer.requests().is_empty());
}
