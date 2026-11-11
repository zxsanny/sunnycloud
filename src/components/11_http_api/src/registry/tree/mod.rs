//! The Tree operations of `api_v1_surface.md` (02 section 3): the account root, folders and nodes, their
//! children by keyset, and revisions with their lookup by content hash. Session or device auth; the account is
//! the principal's.

mod handlers;
pub(super) mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const NODES: &str = "/api/v1/nodes";
const ROOT: &str = "/api/v1/nodes/root";
const NODE: &str = "/api/v1/nodes/{id}";
const CHILDREN: &str = "/api/v1/nodes/{id}/children";
const REVISIONS: &str = "/api/v1/revisions";
const REVISION: &str = "/api/v1/revisions/{id}";
const LOOKUP: &str = "/api/v1/revisions:lookup";

const ID: Param = Param::id(ParamLocation::Path, "id", ParamType::Uuid);
const BY_ID: &[Param] = &[ID];
const CREATE_PARAMS: &[Param] = &[Param::id(ParamLocation::Body, "parent_id", ParamType::Uuid)];
const UPDATE_PARAMS: &[Param] = &[
    ID,
    Param::id(ParamLocation::Body, "parent_id", ParamType::Uuid),
];
const DELETE_PARAMS: &[Param] = &[
    ID,
    Param::id(ParamLocation::Query, "base_revision", ParamType::Uuid),
];
const CHILDREN_PARAMS: &[Param] = &[
    ID,
    Param::query("after", ParamType::String),
    Param::query("limit", ParamType::Integer),
];
const REVISIONS_PARAMS: &[Param] = &[
    Param::query("client_device", ParamType::String),
    Param::query("offset", ParamType::Integer),
    Param::query("limit", ParamType::Integer),
];

const SESSION_OR_DEVICE: AuthClass = AuthClass::SessionOrDevice;

const NOT_FOUND: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];
const CREATE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidName,
    ErrorCode::NameConflict,
    ErrorCode::ParentDeleted,
    ErrorCode::NotFound,
    ErrorCode::NotAFolder,
];
const UPDATE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidName,
    ErrorCode::NameConflict,
    ErrorCode::Cycle,
    ErrorCode::ParentDeleted,
    ErrorCode::NotAFolder,
    ErrorCode::NotFound,
];
const DELETE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::EditedSince,
    ErrorCode::NotFound,
];
const CHILDREN_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAFolder,
    ErrorCode::InvalidCursor,
];
const CURSOR_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::InvalidCursor];
const LOOKUP_ERRORS: &[ErrorCode] = &[ErrorCode::BatchTooLarge];

const GET_ROOT: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    ROOT,
    "TreeReader::get_node",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("The root folder of the caller's account")
.errors(NOT_FOUND)
.response(wire::NODE_VIEW);

const CREATE_FOLDER: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    NODES,
    "TreeService::create_folder",
    SESSION_OR_DEVICE,
    BodyClass::Small,
)
.summary("Create a folder; files are created by commits")
.params(CREATE_PARAMS)
.errors(CREATE_ERRORS)
.request(wire::CREATE_FOLDER_REQUEST)
.response(wire::NODE_VIEW)
.success(201);

const GET_NODE: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    NODE,
    "TreeReader::get_node",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("A node with its current revision")
.params(BY_ID)
.errors(NOT_FOUND)
.response(wire::NODE_VIEW);

const UPDATE_NODE: RouteSpec = RouteSpec::new(
    HttpMethod::Patch,
    NODE,
    "TreeService::update_node",
    SESSION_OR_DEVICE,
    BodyClass::Small,
)
.summary("Rename, move, or both; a move never carries a base revision")
.params(UPDATE_PARAMS)
.errors(UPDATE_ERRORS)
.request(wire::UPDATE_NODE_REQUEST)
.response(wire::NODE_VIEW);

const DELETE_NODE: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    NODE,
    "TreeService::delete_node",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("Delete a node; with `base_revision`, a file edited since is refused")
.params(DELETE_PARAMS)
.errors(DELETE_ERRORS)
.success(204);

const LIST_CHILDREN: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    CHILDREN,
    "TreeReader::list_children",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("The live children of a folder by name key, `after` the key of the last one seen")
.params(CHILDREN_PARAMS)
.errors(CHILDREN_ERRORS)
.response(wire::NODE_LISTING);

const GET_REVISION: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    REVISION,
    "TreeReader::get_revision",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("A revision with its blocklist")
.params(BY_ID)
.errors(NOT_FOUND)
.response(wire::REVISION_VIEW);

const FIND_REVISIONS: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    REVISIONS,
    "TreeService::find_revisions",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("Revisions recorded for a client device, a page at a time")
.params(REVISIONS_PARAMS)
.errors(CURSOR_ERRORS)
.response(wire::REVISION_LIST);

/// A POST that only reads. Its body is up to 1,000 hashes (about 70 KB): above `Small`, inside `Query`.
const LOOKUP_REVISIONS: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    LOOKUP,
    "TreeReader::lookup_revisions",
    SESSION_OR_DEVICE,
    BodyClass::Query,
)
.summary("The current revisions of live files with the given content hashes")
.errors(LOOKUP_ERRORS)
.request(wire::LOOKUP_REQUEST)
.response(wire::REVISION_MATCHES)
.read_only();

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(GET_ROOT, |context, filter| {
            on(filter, handlers::root).with_state(services(context))
        }),
        RouteDef::new(CREATE_FOLDER, |context, filter| {
            on(filter, handlers::create_folder).with_state(services(context))
        }),
        RouteDef::new(GET_NODE, |context, filter| {
            on(filter, handlers::get_node).with_state(services(context))
        }),
        RouteDef::new(UPDATE_NODE, |context, filter| {
            on(filter, handlers::update_node).with_state(services(context))
        }),
        RouteDef::new(DELETE_NODE, |context, filter| {
            on(filter, handlers::delete_node).with_state(services(context))
        }),
        RouteDef::new(LIST_CHILDREN, |context, filter| {
            on(filter, handlers::list_children).with_state(services(context))
        }),
        RouteDef::new(GET_REVISION, |context, filter| {
            on(filter, handlers::get_revision).with_state(services(context))
        }),
        RouteDef::new(FIND_REVISIONS, |context, filter| {
            on(filter, handlers::find_revisions).with_state(services(context))
        }),
        RouteDef::new(LOOKUP_REVISIONS, |context, filter| {
            on(filter, handlers::lookup_revisions).with_state(services(context))
        }),
    ]
}
