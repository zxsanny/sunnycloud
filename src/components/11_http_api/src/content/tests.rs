use std::pin::Pin;
use std::task::{Context, Poll};

use axum::http::{Method, StatusCode, header};
use axum::response::Response;
use contracts::{ByteRange, Disposition, Download, DownloadError, DownloadStatus, StreamError};
use futures_core::Stream;
use http_body_util::BodyExt;
use hyper::body::Bytes;
use sc_error::CorrelationId;
use uuid::Uuid;

use super::content_response;
use super::disposition::attachment;

const ID: CorrelationId = CorrelationId::new(Uuid::from_u128(7));

struct Chunks(Vec<Result<Bytes, StreamError>>);

impl Stream for Chunks {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(if self.0.is_empty() {
            None
        } else {
            Some(self.0.remove(0))
        })
    }
}

fn download(status: DownloadStatus, disposition: Disposition, media_type: &str) -> Download {
    Download {
        status,
        total_size: 1024,
        range: None,
        etag: "blob-1".to_owned(),
        media_type: Some(media_type.to_owned()),
        disposition,
        suggested_name: "a.bin".to_owned(),
        body: Box::pin(Chunks(vec![Ok(Bytes::from_static(b"body"))])),
    }
}

fn header_of(response: &Response, name: header::HeaderName) -> Option<&str> {
    response.headers().get(name).and_then(|v| v.to_str().ok())
}

fn decode(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).expect("hex digits");
            out.push(u8::from_str_radix(hex, 16).expect("hex byte"));
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    out
}

fn decoded_name(name: &str) -> String {
    let value = attachment(name);
    let encoded = value
        .strip_prefix("attachment; filename*=UTF-8''")
        .expect("the attachment prefix");
    String::from_utf8(decode(encoded)).expect("the name is UTF-8")
}

#[test]
fn hostile_names_are_sanitised_into_one_header_line() {
    // Arrange
    let names = [
        "a\"b.txt",
        "line\r\nbreak.txt",
        "..\\x",
        "rtl\u{202E}gpj.exe",
        "../../etc/passwd",
    ];

    // Act
    let values: Vec<String> = names.iter().map(|name| attachment(name)).collect();

    // Assert
    for value in &values {
        assert!(value.is_ascii());
        assert!(!value.contains(['\r', '\n', '"', '/', '\\']));
        assert!(!value.to_lowercase().contains("%e2%80%ae"));
    }
    assert_eq!(values[0], "attachment; filename*=UTF-8''a%22b.txt");
    assert_eq!(decoded_name("..\\x"), ".._x");
    assert_eq!(decoded_name("../../etc/passwd"), ".._.._etc_passwd");
}

#[test]
fn empty_and_dots_only_names_fall_back_to_download() {
    // Arrange
    let names = ["", "   ", "..", "\r\n"];

    // Act
    let decoded: Vec<String> = names.iter().map(|name| decoded_name(name)).collect();

    // Assert
    assert!(decoded.iter().all(|name| name == "download"));
}

#[test]
fn a_long_name_is_cut_on_a_character_boundary() {
    // Arrange
    let name = "ф".repeat(150);

    // Act
    let decoded = decoded_name(&name);

    // Assert
    assert_eq!(decoded.len(), 254);
    assert!(decoded.chars().all(|ch| ch == 'ф'));
}

#[test]
fn a_non_ascii_name_is_percent_encoded_utf8() {
    // Arrange
    let name = "звіт 1.pdf";

    // Act
    let value = attachment(name);

    // Assert
    assert!(value.ends_with("%D0%B7%D0%B2%D1%96%D1%82%201.pdf"));
}

#[test]
fn an_inline_type_outside_the_allowlist_is_rendered_as_an_attachment() {
    // Arrange
    let hostile = download(DownloadStatus::Full, Disposition::Inline, "text/html");

    // Act
    let response = content_response(Ok(hostile), &Method::GET, ID);

    // Assert
    assert_eq!(
        header_of(&response, header::CONTENT_TYPE),
        Some("application/octet-stream")
    );
    assert!(
        header_of(&response, header::CONTENT_DISPOSITION)
            .is_some_and(|value| value.starts_with("attachment;"))
    );
    assert_eq!(
        header_of(&response, header::CONTENT_SECURITY_POLICY),
        Some("sandbox")
    );
}

#[test]
fn an_attachment_is_never_upgraded_to_inline() {
    // Arrange
    let attachment = download(DownloadStatus::Full, Disposition::Attachment, "image/png");

    // Act
    let response = content_response(Ok(attachment), &Method::GET, ID);

    // Assert
    assert_eq!(
        header_of(&response, header::CONTENT_TYPE),
        Some("application/octet-stream")
    );
}

#[test]
fn an_allowlisted_type_in_another_case_is_served_with_the_canonical_type() {
    // Arrange
    let inline = download(DownloadStatus::Full, Disposition::Inline, "Image/PNG");

    // Act
    let response = content_response(Ok(inline), &Method::GET, ID);

    // Assert
    assert_eq!(
        header_of(&response, header::CONTENT_TYPE),
        Some("image/png")
    );
    assert_eq!(
        header_of(&response, header::CONTENT_DISPOSITION),
        Some("inline")
    );
    assert_eq!(header_of(&response, header::CONTENT_SECURITY_POLICY), None);
}

#[test]
fn a_partial_download_carries_its_range_and_length() {
    // Arrange
    let mut partial = download(DownloadStatus::Partial, Disposition::Inline, "image/gif");
    partial.range = Some(ByteRange {
        start: 5,
        end: 1023,
    });

    // Act
    let response = content_response(Ok(partial), &Method::GET, ID);

    // Assert
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        header_of(&response, header::CONTENT_RANGE),
        Some("bytes 5-1023/1024")
    );
    assert_eq!(header_of(&response, header::CONTENT_LENGTH), Some("1019"));
    assert_eq!(header_of(&response, header::ETAG), Some("\"blob-1\""));
}

#[test]
fn a_partial_download_without_a_range_inside_the_total_is_an_opaque_failure() {
    // Arrange
    let mut missing = download(DownloadStatus::Partial, Disposition::Inline, "image/gif");
    missing.range = None;
    let mut outside = download(DownloadStatus::Partial, Disposition::Inline, "image/gif");
    outside.range = Some(ByteRange {
        start: 0,
        end: 1024,
    });

    // Act
    let answers = [missing, outside].map(|d| content_response(Ok(d), &Method::GET, ID));

    // Assert
    for answer in answers {
        assert_eq!(answer.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            header_of(&answer, header::X_CONTENT_TYPE_OPTIONS),
            Some("nosniff")
        );
    }
}

#[tokio::test]
async fn head_and_not_modified_keep_the_headers_and_send_no_body() {
    // Arrange
    let full = download(DownloadStatus::Full, Disposition::Inline, "video/mp4");
    let unchanged = download(
        DownloadStatus::NotModified,
        Disposition::Inline,
        "video/mp4",
    );

    // Act
    let head = content_response(Ok(full), &Method::HEAD, ID);
    let not_modified = content_response(Ok(unchanged), &Method::GET, ID);

    // Assert
    assert_eq!(header_of(&head, header::CONTENT_LENGTH), Some("1024"));
    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(header_of(&not_modified, header::CONTENT_LENGTH), None);
    for response in [head, not_modified] {
        assert_eq!(header_of(&response, header::ACCEPT_RANGES), Some("bytes"));
        assert_eq!(header_of(&response, header::ETAG), Some("\"blob-1\""));
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        assert!(body.is_empty());
    }
}

#[test]
fn a_port_error_is_a_problem_that_keeps_nosniff() {
    // Arrange
    let missing = Err(DownloadError::ContentUnavailable);

    // Act
    let response = content_response(missing, &Method::GET, ID);

    // Assert
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        header_of(&response, header::X_CONTENT_TYPE_OPTIONS),
        Some("nosniff")
    );
}
