use axum::http::StatusCode;
use contracts::DownloadError;

use super::support::*;

#[tokio::test]
async fn not_found_and_content_unavailable_are_404_problems_with_nosniff() {
    // Arrange
    let server = ContentServer::start().await;
    let cases = [
        (DownloadError::NotFound, "not_found"),
        (DownloadError::ContentUnavailable, "content_unavailable"),
    ];

    for (error, code) in cases {
        server.serve(Err(error));

        // Act
        let reply = server.get().await;

        // Assert
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{code}");
        assert_eq!(reply.json()["code"], code);
        assert_eq!(
            reply.header("content-type"),
            Some("application/problem+json")
        );
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    }
    server.stop().await;
}
