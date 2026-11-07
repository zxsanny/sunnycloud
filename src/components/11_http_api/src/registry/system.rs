use axum::http::header::CONTENT_TYPE;
use axum::routing::{MethodFilter, MethodRouter, on};

use super::RouteContext;
use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::spec::{RouteDef, RouteSpec};

const SERVER_INFO: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    "/api/v1/server-info",
    "ServerInfo::value",
    AuthClass::Public,
    BodyClass::None,
)
.summary("Protocol limits and the minimum client version");

const OPENAPI: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    "/api/v1/openapi.json",
    "HttpServer::openapi",
    AuthClass::Public,
    BodyClass::None,
)
.summary("The OpenAPI document of this server");

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(SERVER_INFO, mount_server_info),
        RouteDef::new(OPENAPI, mount_openapi),
    ]
}

/// The body is E-21/c06's: `ServerInfo` has no fields until that task, so the answer is its empty value.
fn mount_server_info(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async {
        ([(CONTENT_TYPE, "application/json")], "{}")
    })
}

fn mount_openapi(context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    let document = context.openapi.to_body();
    on(filter, move || {
        let document = document.clone();
        async move { ([(CONTENT_TYPE, "application/json")], document) }
    })
}
