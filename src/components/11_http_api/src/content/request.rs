//! The request side of a content route: the options 03's open calls take, read from the headers.

use axum::http::{HeaderMap, header};
use contracts::{ByteRange, DownloadOptions};

/// The options of a download by a caller who may not open a retained revision. `Range` is read as one byte
/// range (see [`requested_range`]); the entity tags are handed to 03 as written.
pub(crate) fn download_options(headers: &HeaderMap) -> DownloadOptions {
    DownloadOptions {
        retained_ok: false,
        range: requested_range(headers),
        if_range: text(headers, header::IF_RANGE),
        if_none_match: text(headers, header::IF_NONE_MATCH),
    }
}

/// True when the request starts a download: it has no usable `Range`, or its range starts at byte 0. 06 audits
/// one `share_accessed` per start, however many ranged requests follow.
pub(crate) fn starts_download(headers: &HeaderMap) -> bool {
    requested_range(headers).is_none_or(|range| range.start == 0)
}

/// The one range of `Range: bytes=<start>-<end>` or `bytes=<start>-` (the open end is `u64::MAX`, which 03
/// clamps to the size). A header that is not that form (several ranges, a suffix range `bytes=-n`, another
/// unit, an end before the start) is ignored and the whole content is served, as RFC 9110 allows: 03's
/// `ByteRange` holds two ends, so it cannot carry a suffix range.
fn requested_range(headers: &HeaderMap) -> Option<ByteRange> {
    let value = headers.get(header::RANGE)?.to_str().ok()?;
    let (unit, spec) = value.split_once('=')?;
    if !unit.trim().eq_ignore_ascii_case("bytes") || spec.contains(',') {
        return None;
    }
    let (start, end) = spec.trim().split_once('-')?;
    let start: u64 = start.parse().ok()?;
    let end = if end.is_empty() {
        u64::MAX
    } else {
        end.parse().ok()?
    };
    (start <= end).then_some(ByteRange { start, end })
}

fn text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn ranged(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(
                header::RANGE,
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        headers
    }

    #[test]
    fn a_request_without_a_range_or_with_a_range_from_byte_zero_starts_a_download() {
        // Arrange
        let cases = [
            None,
            Some("bytes=0-99"),
            Some("bytes=0-"),
            Some("bytes=0-0"),
        ];

        // Act
        let started: Vec<bool> = cases
            .into_iter()
            .map(|case| starts_download(&ranged(case)))
            .collect();

        // Assert
        assert_eq!(started, [true; 4]);
    }

    #[test]
    fn a_range_from_any_other_byte_does_not_start_a_download() {
        // Arrange
        let cases = [
            "bytes=1-99",
            "bytes=100-199",
            "bytes=100-",
            "bytes=4194304-",
        ];

        // Act
        let started: Vec<bool> = cases
            .into_iter()
            .map(|case| starts_download(&ranged(Some(case))))
            .collect();

        // Assert
        assert_eq!(started, [false; 4]);
    }

    #[test]
    fn a_header_that_is_not_one_byte_range_is_ignored_so_the_whole_content_is_a_start() {
        // Arrange
        let cases = [
            "bytes=-500",
            "bytes=0-1,5-9",
            "bytes=9-3",
            "items=5-9",
            "bytes=a-b",
            "garbage",
        ];

        // Act
        let (ranges, started): (Vec<_>, Vec<_>) = cases
            .into_iter()
            .map(|case| {
                let headers = ranged(Some(case));
                (download_options(&headers).range, starts_download(&headers))
            })
            .unzip();

        // Assert
        assert_eq!(ranges, vec![None; 6]);
        assert_eq!(started, [true; 6]);
    }

    #[test]
    fn the_range_and_the_validators_reach_the_options_as_written() {
        // Arrange
        let mut headers = ranged(Some("bytes=5-9"));
        headers.insert(header::IF_RANGE, HeaderValue::from_static("\"tag\""));
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("\"other\""));

        // Act
        let options = download_options(&headers);

        // Assert
        assert_eq!(options.range, Some(ByteRange { start: 5, end: 9 }));
        assert_eq!(options.if_range.as_deref(), Some("\"tag\""));
        assert_eq!(options.if_none_match.as_deref(), Some("\"other\""));
        assert!(!options.retained_ok);
    }

    #[test]
    fn an_open_ended_range_ends_at_the_largest_offset() {
        // Arrange
        let headers = ranged(Some("bytes=5-"));

        // Act
        let range = download_options(&headers).range;

        // Assert
        assert_eq!(
            range,
            Some(ByteRange {
                start: 5,
                end: u64::MAX
            })
        );
    }
}
