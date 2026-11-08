//! Layer 6 of the stack of 11 section 5: the body limit and the time limits of a route, by its `BodyClass` and
//! `Timing` (`api_v1_surface.md`, body classes and time limits).
//!
//! It is the innermost layer of a route, in front of the handler and behind the version gate and auth, so a
//! refused caller never has a body byte read. A declared length over the class limit is 413 before any byte is
//! read; a chunked body is cut at the limit; a body idle for too long, or a handler over its request time,
//! ends the request with a reset (the contract lists no status for either).

mod body;
mod layer;
mod plan;

use std::sync::Arc;

use axum::middleware::from_fn_with_state;
use axum::routing::MethodRouter;
use sc_clock::Clock;

use crate::config::HttpConfig;
use crate::metrics::HttpMetrics;
use crate::registry::RouteSpec;
use layer::{Route, enforce};
use plan::Settings;

/// The limits of every route, built once from the configuration and applied to each route with
/// [`guard`](Self::guard).
pub struct BodyLimits {
    settings: Settings,
    metrics: Arc<HttpMetrics>,
    clock: Arc<dyn Clock>,
}

impl BodyLimits {
    pub fn new(
        config: &HttpConfig,
        metrics: Arc<HttpMetrics>,
        clock: Arc<dyn Clock>,
    ) -> Arc<BodyLimits> {
        Arc::new(BodyLimits {
            settings: Settings::from_config(config),
            metrics,
            clock,
        })
    }

    /// Puts the limits of `spec` in front of `handler`. Like auth, it runs only for a request that matched
    /// the route.
    pub(crate) fn guard(self: &Arc<Self>, handler: MethodRouter, spec: &RouteSpec) -> MethodRouter {
        let route = Arc::new(Route {
            limits: Arc::clone(self),
            class: spec.body,
            timing: spec.timing,
        });
        handler.route_layer(from_fn_with_state(route, enforce))
    }
}
