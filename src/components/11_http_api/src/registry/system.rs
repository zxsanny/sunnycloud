use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::routing::{MethodFilter, MethodRouter, on};

use super::RouteContext;
use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::spec::{RouteDef, RouteSpec, SchemaRef};
use crate::server_info::ServerInfo;

const SERVER_INFO: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    "/api/v1/server-info",
    "ServerInfo::value",
    AuthClass::Public,
    BodyClass::None,
)
.summary("Protocol limits and the minimum client version")
.response(SchemaRef {
    name: "ServerInfo",
    build: ServerInfo::schema,
});

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

fn mount_server_info(context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    let body = context.server_info.to_body();
    on(filter, move || {
        let body = body.clone();
        async move {
            (
                [
                    (CONTENT_TYPE, "application/json"),
                    (CACHE_CONTROL, "no-store"),
                ],
                body,
            )
        }
    })
}

fn mount_openapi(context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    let document = context.openapi.to_body();
    on(filter, move || {
        let document = document.clone();
        async move { ([(CONTENT_TYPE, "application/json")], document) }
    })
}
