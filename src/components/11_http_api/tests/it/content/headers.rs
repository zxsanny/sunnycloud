use axum::http::{Method, StatusCode};
use contracts::{Disposition, DownloadStatus};

use super::support::*;

const INLINE: [&str; 7] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
    "video/mp4",
    "video/webm",
];

const HOSTILE: [(&str, &str); 6] = [
    ("text/html", "page.html"),
    ("image/svg+xml", "logo.svg"),
    ("application/xml", "data.xml"),
    ("text/javascript", "app.js"),
    ("application/pdf", "doc.pdf"),
    ("image/gif", "polyglot.gif"),
];

const NAMES: [&str; 5] = [
    "a\"b.txt",
    "line\r\nbreak.txt",
    "..\\x",
    "rtl\u{202E}gpj.exe",
    "ф\u{fe0f}-long-name-ф",
];

#[tokio::test]
async fn the_seven_inline_types_are_served_inline_with_their_own_type() {
    // Arrange
    let server = ContentServer::start().await;

    for media_type in INLINE {
        server.serve(Ok(download(
            DownloadStatus::Full,
            Disposition::Inline,
            media_type,
            "photo",
            KIB,
        )));

        // Act
        let reply = server.get().await;

        // Assert
        assert_eq!(reply.status, StatusCode::OK, "{media_type}");
        assert_eq!(reply.header("content-type"), Some(media_type));
        assert_eq!(reply.header("content-disposition"), Some("inline"));
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(reply.header("content-security-policy"), None);
        assert_eq!(reply.body.len(), KIB);
    }
    server.stop().await;
}

#[tokio::test]
async fn hostile_types_and_names_are_attachments_without_header_injection() {
    // Arrange
    let server = ContentServer::start().await;

    for (media_type, plain_name) in HOSTILE {
        for name in NAMES.iter().copied().chain([plain_name]) {
            server.serve(Ok(download(
                DownloadStatus::Full,
                Disposition::Attachment,
                media_type,
                name,
                KIB,
            )));

            // Act
            let reply = server.get().await;

            // Assert
            let disposition = reply.header("content-disposition").expect("disposition");
            assert_eq!(
                reply.header("content-type"),
                Some("application/octet-stream")
            );
            assert!(
                disposition.starts_with("attachment; filename*=UTF-8''"),
                "{disposition}"
            );
            assert!(
                !disposition.contains(['\r', '\n', '"', '/', '\\']),
                "{disposition}"
            );
            assert!(
                !disposition.to_lowercase().contains("%e2%80%ae"),
                "{disposition}"
            );
            assert_eq!(reply.header("content-security-policy"), Some("sandbox"));
            assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        }
    }
    server.stop().await;
}

#[tokio::test]
async fn a_300_byte_name_is_cut_and_a_quote_is_escaped() {
    // Arrange
    let server = ContentServer::start().await;
    let long = "ж".repeat(150);
    for name in [long.as_str(), "a\"b.txt"] {
        server.serve(Ok(download(
            DownloadStatus::Full,
            Disposition::Attachment,
            "application/pdf",
            name,
            KIB,
        )));
    }

    // Act
    let cut = server.get().await;
    let quoted = server.get().await;

    // Assert
    let cut = cut.header("content-disposition").expect("disposition");
    assert_eq!(cut.matches("%D0%B6").count(), 127);
    assert_eq!(
        quoted.header("content-disposition"),
        Some("attachment; filename*=UTF-8''a%22b.txt")
    );
    server.stop().await;
}

#[tokio::test]
async fn head_answers_with_the_same_headers_and_no_body() {
    // Arrange
    let server = ContentServer::start().await;
    server.serve(Ok(download(
        DownloadStatus::Full,
        Disposition::Inline,
        "image/png",
        "photo",
        KIB,
    )));

    // Act
    let reply = server.h1(Method::HEAD).await.expect("a HEAD answer");

    // Assert
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header("content-type"), Some("image/png"));
    assert_eq!(reply.header("content-length"), Some("1024"));
    assert_eq!(reply.header("etag"), Some("\"blob-etag\""));
    assert!(reply.body.is_empty());
    server.stop().await;
}
