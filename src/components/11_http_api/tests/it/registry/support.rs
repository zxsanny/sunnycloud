use axum::http::StatusCode;
use axum::routing::{MethodFilter, MethodRouter, on};
use sunnycloud_http_api::registry::{
    RouteContext, RouteDef, RouteSpec, Routers, document, mount, production_routes,
};

use crate::harness::Script;
use crate::harness::router_server::route_context_with;

pub fn no_content(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async { StatusCode::NO_CONTENT })
}

pub fn def(spec: RouteSpec) -> RouteDef {
    RouteDef::new(spec, no_content)
}

/// The surface the registry tests run over: every production route (nested paths, the colon forms, a block body,
/// HEAD beside GET, the callback that mutates by protocol, and the share grant, admin and worker groups).
pub fn sample_surface() -> Vec<RouteDef> {
    production_routes()
}

pub fn sample_routers(
    defs: &[RouteDef],
) -> (Routers, sunnycloud_http_api::registry::OpenApiDocument) {
    sample_routers_with(defs, &Script::default())
}

/// [`sample_routers`] whose ports read `script`.
pub fn sample_routers_with(
    defs: &[RouteDef],
    script: &Script,
) -> (Routers, sunnycloud_http_api::registry::OpenApiDocument) {
    let openapi = document(defs);
    let routers = mount(defs, &route_context_with(openapi.clone(), script), None);
    (routers, openapi)
}

/// A path template with every variable replaced by a value the router accepts.
pub fn concrete(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') {
                "7"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}
