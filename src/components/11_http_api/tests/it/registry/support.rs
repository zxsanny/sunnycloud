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
const NODE_CONTENT: &[Param] = &[Param::id(ParamLocation::Path, "node_id", ParamType::Uuid)];
const BLOCK: &[Param] = &[
    Param::id(ParamLocation::Path, "upload_id", ParamType::Uuid),
    Param::path("index", ParamType::Integer),
];
const CHILDREN: &[Param] = &[
    NODE_ID,
    Param::id(ParamLocation::Query, "after", ParamType::Uuid),
    Param::query("limit", ParamType::Integer),
];
const CALLBACK: &[Param] = &[Param::path("provider", ParamType::String)];

const SOD: AuthClass = AuthClass::SessionOrDevice;

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

/// A slice of the real surface covering every shape the generators handle: nested paths, a colon form, a
/// block body, HEAD beside GET, a GET that mutates and admin, beside the production routes (the share grant and
/// worker groups among them).
pub fn sample_surface() -> Vec<RouteDef> {
    use HttpMethod::*;
    let specs = [
        op(
            Get,
            "/api/v1/nodes/root",
            "TreeReader::get_node",
            SOD,
            BodyClass::None,
        ),
        op(
            Post,
            "/api/v1/nodes",
            "TreeService::create_folder",
            SOD,
            BodyClass::Small,
        )
        .success(201),
        op(
            Get,
            "/api/v1/nodes/{id}",
            "TreeReader::get_node",
            SOD,
            BodyClass::None,
        )
        .params(NODE),
        op(
            Patch,
            "/api/v1/nodes/{id}",
            "TreeService::update_node",
            SOD,
            BodyClass::Small,
        )
        .params(NODE),
        op(
            Delete,
            "/api/v1/nodes/{id}",
            "TreeService::delete_node",
            SOD,
            BodyClass::None,
        )
        .params(NODE),
        op(
            Get,
            "/api/v1/nodes/{id}/children",
            "TreeReader::list_children",
            SOD,
            BodyClass::None,
        )
        .params(CHILDREN),
        op(
            Put,
            "/api/v1/uploads/{upload_id}/blocks/{index}",
            "UploadService::put_block",
            SOD,
            BodyClass::Block,
        )
        .params(BLOCK)
        .success(204),
        op(
            Post,
            "/api/v1/commits:batch",
            "CommitService::commit_batch",
            SOD,
            BodyClass::Batch,
        ),
        op(
            Post,
            "/api/v1/revisions:lookup",
            "TreeReader::lookup_revisions",
            SOD,
            BodyClass::Query,
        )
        .read_only(),
        op(
            Get,
            "/api/v1/nodes/{node_id}/content",
            "DownloadService::open_revision_content",
            SOD,
            BodyClass::None,
        )
        .params(NODE_CONTENT),
        op(
            Head,
            "/api/v1/nodes/{node_id}/content",
            "DownloadService::open_revision_content",
            SOD,
            BodyClass::None,
        )
        .params(NODE_CONTENT),
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
