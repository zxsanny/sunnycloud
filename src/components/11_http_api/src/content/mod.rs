//! The one mapping from 03's `Download` to an HTTP answer (`content_serving_headers.md`), used by every route
//! that streams stored bytes: node and revision content, share content, thumbnails and transcodes.
//!
//! The inline-or-attachment decision is 03's; this module only renders it, and renders an `Inline` answer
//! whose type is not on the allowlist as an attachment, never the other way round (AC-47).

mod disposition;
#[cfg(test)]
mod tests;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use contracts::{ByteRange, Download, DownloadError, DownloadStatus, PlaintextStream};
use http_body_util::BodyExt;
use sc_error::{CorrelationId, DomainError};
use sc_observability::log_error;

use crate::problem::ApiProblem;

/// A `Download` that cannot be rendered: 03 broke its own contract. Answered as an opaque 500 and logged.
#[derive(Debug, thiserror::Error)]
enum InconsistentDownload {
    #[error("download has a negative total size {0}")]
    TotalSize(i64),
    #[error("partial download without a range inside its total size")]
    PartialRange,
    #[error("entity tag is not a header value")]
    ETag(#[source] header::InvalidHeaderValue),
    #[error("file name header is not a header value")]
    FileName(#[source] header::InvalidHeaderValue),
    #[error("content range is not a header value")]
    ContentRange(#[source] header::InvalidHeaderValue),
}

/// Answers a content route from the result of 03's open call.
///
/// A port error is answered by [`ApiProblem`] (404, 416 and the rest, with `nosniff`). `HEAD` and `304` carry
/// the headers of the same call and no body. A body error after the headers is returned to the server, which
/// closes an HTTP/1.1 connection and resets an HTTP/2 stream, so a failed stream never ends as a clean short
/// answer.
pub fn content_response(
    result: Result<Download, DownloadError>,
    method: &Method,
    request_id: CorrelationId,
) -> Response {
    let download = match result {
        Ok(download) => download,
        Err(DownloadError::RangeNotSatisfiable { total_size }) => {
            return unsatisfiable(total_size, request_id);
        }
        Err(error) => return ApiProblem::from_port(error, request_id).into_response(),
    };
    render(download, method, request_id)
        .unwrap_or_else(|inconsistent| inconsistent_answer(inconsistent, request_id))
}

fn inconsistent_answer(inconsistent: InconsistentDownload, request_id: CorrelationId) -> Response {
    ApiProblem::new(
        DomainError::unexpected(inconsistent, request_id),
        request_id,
    )
    .into_response()
}

/// The 416 problem, with the size of the whole content so a client can tell what it may ask for.
fn unsatisfiable(total_size: i64, request_id: CorrelationId) -> Response {
    let Ok(total) = u64::try_from(total_size) else {
        return inconsistent_answer(InconsistentDownload::TotalSize(total_size), request_id);
    };
    let value = match HeaderValue::try_from(format!("bytes */{total}")) {
        Ok(value) => value,
        Err(error) => {
            return inconsistent_answer(InconsistentDownload::ContentRange(error), request_id);
        }
    };
    let mut response = ApiProblem::from_port(
        DownloadError::RangeNotSatisfiable { total_size },
        request_id,
    )
    .into_response();
    response.headers_mut().insert(header::CONTENT_RANGE, value);
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response
}

fn render(
    download: Download,
    method: &Method,
    request_id: CorrelationId,
) -> Result<Response, InconsistentDownload> {
    let Download {
        status,
        total_size,
        range,
        etag,
        media_type,
        disposition,
        suggested_name,
        body,
    } = download;
    let total =
        u64::try_from(total_size).map_err(|_| InconsistentDownload::TotalSize(total_size))?;
    let mut headers = HeaderMap::new();
    disposition::apply(
        &mut headers,
        disposition,
        media_type.as_deref(),
        &suggested_name,
    )?;
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private"));
    headers.insert(header::ETAG, entity_tag(&etag)?);
    let (status, length) = match status {
        DownloadStatus::Full => (StatusCode::OK, Some(total)),
        DownloadStatus::Partial => {
            let served = range
                .filter(|range| range.start <= range.end && range.end < total)
                .ok_or(InconsistentDownload::PartialRange)?;
            headers.insert(header::CONTENT_RANGE, content_range(served, total)?);
            (
                StatusCode::PARTIAL_CONTENT,
                Some(served.end - served.start + 1),
            )
        }
        DownloadStatus::NotModified => (StatusCode::NOT_MODIFIED, None),
    };
    if let Some(length) = length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    let body = if length.is_some() && method != Method::HEAD {
        stream(body, request_id)
    } else {
        Body::empty()
    };
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    Ok(response)
}

/// The strong tag as a quoted header value; a tag 03 already quoted is kept.
fn entity_tag(etag: &str) -> Result<HeaderValue, InconsistentDownload> {
    let quoted = if etag.starts_with('"') {
        etag.to_owned()
    } else {
        format!("\"{etag}\"")
    };
    HeaderValue::try_from(quoted).map_err(InconsistentDownload::ETag)
}

fn content_range(range: ByteRange, total: u64) -> Result<HeaderValue, InconsistentDownload> {
    HeaderValue::try_from(format!("bytes {}-{}/{total}", range.start, range.end))
        .map_err(InconsistentDownload::ContentRange)
}

/// The body as it is read, without buffering. A failure is logged here because the server only closes or
/// resets the connection and keeps no record of why.
pub(crate) fn stream(body: PlaintextStream, request_id: CorrelationId) -> Body {
    Body::new(Body::from_stream(body).map_err(move |failure| {
        log_error!(
            "content stream failed after the headers were sent",
            request_id = request_id,
            err = &failure
        );
        failure
    }))
}
