use axum::http::StatusCode;
use axum::routing::{MethodFilter, MethodRouter, on};
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, Param, ParamLocation, ParamType, RouteContext, RouteDef,
    RouteSpec, Routers, document, mount, production_routes,
};

use crate::harness::Script;
use crate::harness::router_server::route_context_with;

pub fn no_content(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async { StatusCode::NO_CONTENT })
}

const NODE_ID: Param = Param::id(ParamLocation::Path, "id", ParamType::Uuid);
const NODE: &[Param] = &[NODE_ID];
const CALLBACK: &[Param] = &[Param::path("provider", ParamType::String)];

pub fn def(spec: RouteSpec) -> RouteDef {
    RouteDef::new(spec, no_content)
}

fn op(
    method: HttpMethod,
    path: &'static str,
    owner: &'static str,
    auth: AuthClass,
    body: BodyClass,
) -> RouteSpec {
    RouteSpec::new(method, path, owner, auth, body).summary("A test operation")
}

/// The two shapes no production group registers yet (a GET that mutates by protocol, and an admin route), beside
/// the production routes: nested paths, the colon forms, a block body, HEAD beside GET and the share grant and
/// worker groups come from the production registry itself.
pub fn sample_surface() -> Vec<RouteDef> {
    use HttpMethod::*;
    let specs = [
        op(
            Get,
            "/api/v1/auth/providers/{provider}/callback",
            "SignInService::complete",
            AuthClass::Public,
            BodyClass::None,
        )
        .params(CALLBACK)
        .mutating(),
        op(
            Delete,
            "/api/v1/admin/invites/{id}",
            "InviteService::revoke",
            AuthClass::Admin,
            BodyClass::None,
        )
        .params(NODE),
    ];
    let mut defs = production_routes();
    defs.extend(specs.into_iter().map(def));
    defs
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
