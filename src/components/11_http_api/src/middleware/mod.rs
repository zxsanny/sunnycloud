//! Layers 1 to 5 of the stack of 11 section 5 (outermost first), for the public and internal routers:
//! panic catcher, client IP, request ID, metrics and log, security headers. The version gate, auth, the body and time limits
//! (innermost) and CSRF sit inside them, per route.

mod client_ip;
mod headers;
mod observe;
mod panic;
mod request_id;

use std::sync::Arc;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
use sc_clock::Clock;
use sc_error::{CorrelationId, DomainError};

pub use client_ip::ClientIp;
pub(crate) use client_ip::PeerAddr;
use client_ip::TrustedProxies;
use observe::RouteLabels;

use crate::config::HttpConfig;
use crate::error::HttpServerError;
use crate::metrics::HttpMetrics;
use crate::problem::ApiProblem;

/// What the five layers share.
struct Layers {
    trusted: TrustedProxies,
    routes: RouteLabels,
    metrics: Arc<HttpMetrics>,
    clock: Arc<dyn Clock>,
}

impl Layers {
    fn fresh_id(&self) -> CorrelationId {
        CorrelationId::new(sc_ids::new_id(self.clock.as_ref()))
    }
}

/// The five outer layers, built once from the configuration and applied to each router with [`wrap`](Self::wrap).
#[derive(Clone)]
pub struct Middleware {
    layers: Arc<Layers>,
}

impl Middleware {
    /// `templates` are the route templates of the routers to wrap: the only values a route label can take
    /// (anything else is `unmatched`). `InvalidConfig` when a `trusted_proxies` entry is not `address/prefix`.
    pub fn new(
        config: &HttpConfig,
        clock: Arc<dyn Clock>,
        metrics: Arc<HttpMetrics>,
        templates: impl IntoIterator<Item = &'static str>,
    ) -> Result<Middleware, HttpServerError> {
        Ok(Middleware {
            layers: Arc::new(Layers {
                trusted: TrustedProxies::parse(&config.trusted_proxies)?,
                routes: RouteLabels::new(templates),
                metrics,
                clock,
            }),
        })
    }

    /// Wraps every route and the fallback of `router`. Added innermost first, so the panic catcher is outermost.
    pub fn wrap(&self, router: Router) -> Router {
        let layers = &self.layers;
        router
            .layer(axum::middleware::from_fn(headers::security_headers))
            .layer(from_fn_with_state(Arc::clone(layers), observe::observe))
            .layer(from_fn_with_state(
                Arc::clone(layers),
                request_id::request_id,
            ))
            .layer(from_fn_with_state(Arc::clone(layers), client_ip::client_ip))
            .layer(from_fn_with_state(Arc::clone(layers), panic::catch_panic))
    }
}

/// The 500 problem answer of a failure the stack itself hit. It bypasses layer 5, so it applies the headers.
fn internal_failure(request_id: CorrelationId, reason: &'static str) -> Response {
    let mut response =
        ApiProblem::new(DomainError::unexpected(reason, request_id), request_id).into_response();
    headers::apply(response.headers_mut());
    response
}
