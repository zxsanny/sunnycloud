use axum::http::{Method, StatusCode};
use contracts::{Disposition, DownloadStatus, StreamError};

use super::support::*;

/// 10 MiB declared, 3 MiB delivered, then the stream fails.
fn failing_download() -> contracts::Download {
    let mut failing = download(
        DownloadStatus::Full,
        Disposition::Attachment,
        "application/pdf",
        "big.pdf",
        10 * MIB,
    );
    failing.body = chunked(3 * MIB, Some(StreamError::Corrupt));
    failing
}

#[tokio::test]
async fn a_failed_stream_closes_the_http1_connection_before_content_length() {
    // Arrange
    let server = ContentServer::start().await;
    server.serve(Ok(failing_download()));

    // Act
    let outcome = server.h1(Method::GET).await;

    // Assert
    let failure = outcome.expect_err("the client must see an error, never a clean short body");
    assert_eq!(
        failure.status,
        Some(StatusCode::OK),
        "the headers were sent before the body failed"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_failed_stream_resets_the_http2_stream() {
    // Arrange
    let server = ContentServer::start().await;
    server.serve(Ok(failing_download()));

    // Act
    let outcome = server.h2(Method::GET).await;

    // Assert
    let failure = outcome.expect_err("the client must see an error, never a clean short body");
    assert_eq!(
        failure.status,
        Some(StatusCode::OK),
        "the headers were sent before the stream was reset"
    );
    assert!(
        format!("{:?}", failure.error).contains("Reset("),
        "expected RST_STREAM, got {:?}",
        failure.error
    );
    server.stop().await;
}

#[tokio::test]
async fn a_complete_stream_ends_cleanly_on_both_protocols() {
    // Arrange
    let server = ContentServer::start().await;
    for _ in 0..2 {
        server.serve(Ok(download(
            DownloadStatus::Full,
            Disposition::Attachment,
            "application/pdf",
            "big.pdf",
            3 * MIB,
        )));
    }

    // Act
    let h1 = server.h1(Method::GET).await.expect("h1 answer");
    let h2 = server.h2(Method::GET).await.expect("h2 answer");

    // Assert
    assert_eq!(h1.body.len(), 3 * MIB);
    assert_eq!(h2.body.len(), 3 * MIB);
    server.stop().await;
}
