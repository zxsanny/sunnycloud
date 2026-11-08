//! Layer 9 of the stack of 11 section 5: the CSRF origin check (AC-58, `api_v1_surface.md`).
//!
//! It runs after auth, which records whether the credential was the session cookie, and before the body
//! limits, so a refused request never has a body byte read. The public origin is `public_url` alone, never
//! the `Host` header.

mod origin;

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::ORIGIN;
use axum::http::{HeaderMap, HeaderName};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use sc_clock::Clock;
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};

use crate::auth::{Authenticated, CredentialSource};
use crate::error::HttpServerError;
use crate::problem::ApiProblem;
use crate::registry::HttpMethod;

const SEC_FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");
const SAME_ORIGIN: &[u8] = b"same-origin";

/// Refuses a cookie-authenticated POST, PUT, PATCH or DELETE that does not prove it came from the web app's
/// own origin. The method decides, not the route's state-changing flag, so a mutation registered with the
/// wrong flag is still checked.
pub struct CsrfGuard {
    public_origin: String,
    clock: Arc<dyn Clock>,
}

impl CsrfGuard {
    /// `InvalidConfig` when `public_url` is not a bare `http(s)://host[:port]` origin.
    pub fn new(public_url: &str, clock: Arc<dyn Clock>) -> Result<Arc<CsrfGuard>, HttpServerError> {
        let public_origin = origin::from_public_url(public_url).ok_or_else(|| {
            HttpServerError::InvalidConfig(
                "public_url must be an http(s) origin: scheme, host and optional port".to_owned(),
            )
        })?;
        Ok(Arc::new(CsrfGuard {
            public_origin,
            clock,
        }))
    }

    /// Puts the check in front of `handler` and behind that route's auth. GET and HEAD routes get none.
    pub(crate) fn guard(
        self: &Arc<Self>,
        handler: MethodRouter,
        method: HttpMethod,
    ) -> MethodRouter {
        if matches!(method, HttpMethod::Get | HttpMethod::Head) {
            return handler;
        }
        handler.route_layer(from_fn_with_state(Arc::clone(self), check))
    }

    /// True for `Sec-Fetch-Site: same-origin` or an `Origin` equal to the public origin. A repeated header
    /// proves nothing.
    fn proves_same_origin(&self, headers: &HeaderMap) -> bool {
        single(headers, &SEC_FETCH_SITE).is_some_and(|site| site.as_bytes() == SAME_ORIGIN)
            || single(headers, &ORIGIN)
                .and_then(|value| value.to_str().ok())
                .and_then(origin::canonical)
                .is_some_and(|origin| origin == self.public_origin)
    }
}

fn single<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a axum::http::HeaderValue> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

async fn check(State(guard): State<Arc<CsrfGuard>>, request: Request, next: Next) -> Response {
    let by_cookie = request
        .extensions()
        .get::<Authenticated>()
        .is_some_and(|caller| caller.source == CredentialSource::Cookie);
    if !by_cookie || guard.proves_same_origin(request.headers()) {
        return next.run(request).await;
    }
    let request_id = request
        .extensions()
        .get::<CorrelationId>()
        .copied()
        .unwrap_or_else(|| CorrelationId::new(sc_ids::new_id(guard.clock.as_ref())));
    let error = BusinessError::new(ErrorCode::CsrfRejected, BusinessParams::default());
    ApiProblem::new(DomainError::Business(error), request_id).into_response()
}

#[cfg(test)]
mod tests;
