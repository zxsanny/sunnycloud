use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

const HSTS: HeaderName = header::STRICT_TRANSPORT_SECURITY;
const HSTS_VALUE: &str = "max-age=31536000";

/// Layer 5. Adds the fixed headers of `api_v1_surface.md` to every response.
///
/// `Cache-Control: no-store` is the default: a route that sets its own value (content routes set `private`)
/// keeps it. The other three are fixed and replace whatever the handler wrote. No `Access-Control-*` header
/// is ever added here, so `/api` answers no CORS preflight.
pub(super) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    apply(response.headers_mut());
    response
}

/// Also used for the answers that bypass layer 5 (a caught panic, a failure before the id layer).
pub(super) fn apply(headers: &mut HeaderMap) {
    headers.insert(HSTS, HeaderValue::from_static(HSTS_VALUE));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixed_headers_replace_a_handler_value_and_cache_control_defaults() {
        // Arrange
        let mut headers = HeaderMap::new();
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("other"),
        );

        // Act
        apply(&mut headers);

        // Assert
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[HSTS], "max-age=31536000");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn a_cache_control_set_by_the_route_is_kept() {
        // Arrange
        let mut headers = HeaderMap::new();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private"));

        // Act
        apply(&mut headers);

        // Assert
        assert_eq!(headers[header::CACHE_CONTROL], "private");
    }
}
