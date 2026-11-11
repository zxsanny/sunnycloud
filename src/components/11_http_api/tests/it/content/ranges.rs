use axum::http::{Method, StatusCode};
use contracts::{ByteRange, Disposition, DownloadError, DownloadStatus};

use super::support::*;

fn partial(range: ByteRange) -> contracts::Download {
    let length = usize::try_from(range.end - range.start + 1).expect("range fits");
    let mut served = download(
        DownloadStatus::Partial,
        Disposition::Inline,
        "image/jpeg",
        "photo",
        KIB,
    );
    served.range = Some(range);
    served.body = chunked(length, None);
    served
}

fn assert_validators(reply: &Reply) {
    assert_eq!(reply.header("accept-ranges"), Some("bytes"));
    assert_eq!(reply.header("etag"), Some("\"blob-etag\""));
    assert_eq!(reply.header("cache-control"), Some("private"));
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
}

#[tokio::test]
async fn satisfiable_ranges_are_206_with_a_matching_content_range() {
    // Arrange
    let server = ContentServer::start().await;
    let cases = [
        (0, 0, "bytes 0-0/1024", 1),
        (5, 1023, "bytes 5-1023/1024", 1019),
        (1021, 1023, "bytes 1021-1023/1024", 3),
    ];

    for (start, end, content_range, length) in cases {
        server.serve(Ok(partial(ByteRange { start, end })));

        // Act
        let reply = server.get().await;

        // Assert
        assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT, "{content_range}");
        assert_eq!(reply.header("content-range"), Some(content_range));
        assert_eq!(reply.body.len(), length);
        assert_validators(&reply);
    }
    server.stop().await;
}

#[tokio::test]
async fn a_matching_if_none_match_is_304_without_a_body() {
    // Arrange
    let server = ContentServer::start().await;
    server.serve(Ok(download(
        DownloadStatus::NotModified,
        Disposition::Inline,
        "image/jpeg",
        "photo",
        KIB,
    )));

    // Act
    let reply = server.get().await;

    // Assert
    assert_eq!(reply.status, StatusCode::NOT_MODIFIED);
    assert!(reply.body.is_empty());
    assert_validators(&reply);
    server.stop().await;
}

#[tokio::test]
async fn an_unsatisfiable_range_is_a_416_problem_that_keeps_nosniff() {
    // Arrange
    let server = ContentServer::start().await;
    server.serve(Err(DownloadError::RangeNotSatisfiable));

    // Act
    let reply = server.h1(Method::GET).await.expect("an answer");

    // Assert
    assert_eq!(reply.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(reply.json()["code"], "range_not_satisfiable");
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    server.stop().await;
}
