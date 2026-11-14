//! The serving rules of `download_service.md`, free of I/O: the validators, the one range, the disposition, and
//! the bodies a download hands out (none for `NotModified`, counted bytes otherwise).
//!
//! 11 reads `Range` as one `start-end` or `start-` range (an open end is `u64::MAX`) and ignores every other form
//! (several ranges, a suffix range), so 03 sees at most one range. The entity tags come as written in the request.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{ByteRange, Disposition, PlaintextStream, StreamError};
use futures_core::Stream;

use crate::upload_metrics::UploadMetrics;

/// The sniffed types served inline (AC-47); every other type, and no type, is an attachment.
const INLINE_TYPES: [&str; 7] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
    "video/mp4",
    "video/webm",
];

/// What the request asks for, once the validators are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Serve {
    NotModified,
    Full,
    Partial(ByteRange),
    /// The range starts at or past the end of the content.
    Unsatisfiable,
}

/// `If-None-Match` first (equal: `NotModified`), then `If-Range` (a tag other than the strong current one: the
/// range is ignored), then the range: its end is clamped to the content, a start at or past the end is
/// unsatisfiable.
pub(crate) fn decide(
    etag: &str,
    total_size: u64,
    range: Option<ByteRange>,
    if_range: Option<&str>,
    if_none_match: Option<&str>,
) -> Serve {
    if if_none_match.is_some_and(|header| none_match(header, etag)) {
        return Serve::NotModified;
    }
    if if_range.is_some_and(|tag| !strong_match(tag, etag)) {
        return Serve::Full;
    }
    let Some(range) = range else {
        return Serve::Full;
    };
    if range.start > range.end {
        return Serve::Full;
    }
    if range.start >= total_size {
        return Serve::Unsatisfiable;
    }
    Serve::Partial(ByteRange {
        start: range.start,
        end: range.end.min(total_size - 1),
    })
}

/// Weak comparison over a list of tags, or `*`.
fn none_match(header: &str, etag: &str) -> bool {
    let header = header.trim();
    header == "*"
        || header.split(',').any(|tag| {
            let tag = tag.trim();
            unquoted(tag.strip_prefix("W/").unwrap_or(tag)) == etag
        })
}

/// Strong comparison: a weak tag or a date never matches.
fn strong_match(tag: &str, etag: &str) -> bool {
    let tag = tag.trim();
    !tag.starts_with("W/") && unquoted(tag) == etag
}

fn unquoted(tag: &str) -> &str {
    tag.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(tag)
}

/// `Inline` only for the allowlisted sniffed types; the file name is never consulted.
pub(crate) fn disposition(media_type: Option<&str>) -> Disposition {
    match media_type {
        Some(media_type) if INLINE_TYPES.contains(&media_type) => Disposition::Inline,
        _ => Disposition::Attachment,
    }
}

/// The body of `NotModified`.
struct NoBody;

impl Stream for NoBody {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

pub(crate) fn no_body() -> PlaintextStream {
    Box::pin(NoBody)
}

/// A body that adds each chunk it hands out to `download_bytes_total`.
struct Counted {
    inner: PlaintextStream,
    metrics: Arc<UploadMetrics>,
}

impl Stream for Counted {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let polled = self.inner.as_mut().poll_next(cx);
        if let Poll::Ready(Some(Ok(chunk))) = &polled {
            self.metrics.downloaded(chunk.len() as u64);
        }
        polled
    }
}

pub(crate) fn counted(
    body: PlaintextStream,
    metrics: Option<&Arc<UploadMetrics>>,
) -> PlaintextStream {
    match metrics {
        Some(metrics) => Box::pin(Counted {
            inner: body,
            metrics: Arc::clone(metrics),
        }),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETAG: &str = "0193a2b4-0000-7000-8000-000000000001";
    const TOTAL: u64 = 5 * 1024 * 1024;

    fn range(start: u64, end: u64) -> Option<ByteRange> {
        Some(ByteRange { start, end })
    }

    #[test]
    fn decide_one_range_is_partial_with_its_end_clamped_to_the_content() {
        // Arrange
        let cases = [
            (range(0, 0), ByteRange { start: 0, end: 0 }),
            (
                range(4_194_304, u64::MAX),
                ByteRange {
                    start: 4_194_304,
                    end: TOTAL - 1,
                },
            ),
            (
                range(TOTAL - 4096, TOTAL + 10),
                ByteRange {
                    start: TOTAL - 4096,
                    end: TOTAL - 1,
                },
            ),
        ];

        for (asked, served) in cases {
            // Act
            let serve = decide(ETAG, TOTAL, asked, None, None);

            // Assert
            assert_eq!(serve, Serve::Partial(served), "{asked:?}");
        }
    }

    #[test]
    fn decide_no_range_or_an_inverted_one_is_full() {
        // Arrange
        let cases = [None, range(9, 3)];

        for asked in cases {
            // Act
            let serve = decide(ETAG, TOTAL, asked, None, None);

            // Assert
            assert_eq!(serve, Serve::Full, "{asked:?}");
        }
    }

    #[test]
    fn decide_a_start_at_or_past_the_end_is_unsatisfiable() {
        // Arrange
        let cases = [
            (TOTAL, range(TOTAL, u64::MAX)),
            (TOTAL, range(9_999_999, u64::MAX)),
            (0, range(0, 0)),
        ];

        for (total, asked) in cases {
            // Act
            let serve = decide(ETAG, total, asked, None, None);

            // Assert
            assert_eq!(serve, Serve::Unsatisfiable, "{total} {asked:?}");
        }
    }

    #[test]
    fn decide_if_range_serves_the_range_only_for_the_strong_current_tag() {
        // Arrange
        let current = format!("\"{ETAG}\"");
        let weak = format!("W/\"{ETAG}\"");
        let cases = [
            (
                current.as_str(),
                Serve::Partial(ByteRange { start: 0, end: 9 }),
            ),
            (ETAG, Serve::Partial(ByteRange { start: 0, end: 9 })),
            ("\"stale\"", Serve::Full),
            (weak.as_str(), Serve::Full),
            ("Wed, 21 Oct 2026 07:28:00 GMT", Serve::Full),
        ];

        for (if_range, expected) in cases {
            // Act
            let serve = decide(ETAG, TOTAL, range(0, 9), Some(if_range), None);

            // Assert
            assert_eq!(serve, expected, "{if_range}");
        }
    }

    #[test]
    fn decide_if_none_match_with_the_current_tag_is_not_modified() {
        // Arrange
        let listed = format!("\"other\", W/\"{ETAG}\"");
        let matching = [format!("\"{ETAG}\""), listed, "*".to_owned()];

        for header in &matching {
            // Act
            let serve = decide(ETAG, TOTAL, range(0, 9), None, Some(header));

            // Assert
            assert_eq!(serve, Serve::NotModified, "{header}");
        }
        let other = decide(ETAG, TOTAL, None, None, Some("\"other\""));
        assert_eq!(other, Serve::Full);
    }

    #[test]
    fn disposition_is_inline_for_the_seven_allowlisted_types_only() {
        // Arrange
        let attachments = [
            Some("text/html"),
            Some("image/svg+xml"),
            Some("application/xml"),
            Some("text/javascript"),
            Some("application/pdf"),
            Some("image/heic"),
            Some("video/quicktime"),
            Some("IMAGE/JPEG"),
            None,
        ];

        // Act
        let inline: Vec<Disposition> = INLINE_TYPES
            .iter()
            .map(|media_type| disposition(Some(media_type)))
            .collect();
        let attached: Vec<Disposition> = attachments.iter().map(|m| disposition(*m)).collect();

        // Assert
        assert!(inline.iter().all(|d| *d == Disposition::Inline));
        assert!(attached.iter().all(|d| *d == Disposition::Attachment));
    }
}
