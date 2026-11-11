//! The Sharing operations of `api_v1_surface.md` (06 section 3): the owner's routes to create, list and revoke
//! a link, and the visitor's routes to open it, verify its password and read its content. The share thumbnail
//! of the same table is not registered, see `api_v1_surface.md` open point 5.

mod grant;
mod owner;
mod visitor;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const SHARES: &str = "/api/v1/shares";
const SHARE: &str = "/api/v1/shares/{id}";
const PUBLIC_SHARE: &str = "/api/v1/public-shares/{token}";
const PASSWORD: &str = "/api/v1/public-shares/{token}/password-verifications";
const CONTENT: &str = "/api/v1/public-shares/{token}/content";

const SHARE_ID: &[Param] = &[Param::id(ParamLocation::Path, "id", ParamType::Uuid)];
const TOKEN: &[Param] = &[Param::id(ParamLocation::Path, "token", ParamType::String)];
const CREATE_PARAMS: &[Param] = &[Param::id(ParamLocation::Body, "node_id", ParamType::Uuid)];
const LIST_PARAMS: &[Param] = &[Param::id(ParamLocation::Query, "node_id", ParamType::Uuid)];

const CREATE_ERRORS: &[ErrorCode] = &[ErrorCode::NotFound, ErrorCode::TargetNotShareable];
const LIST_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest];
const REVOKE_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];
const VISITOR_ERRORS: &[ErrorCode] = &[
    ErrorCode::NotFound,
    ErrorCode::PasswordRequired,
    ErrorCode::RateLimited,
];
const CONTENT_ERRORS: &[ErrorCode] = &[
    ErrorCode::NotFound,
    ErrorCode::RateLimited,
    ErrorCode::RangeNotSatisfiable,
    ErrorCode::ContentUnavailable,
];

const CREATE: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    SHARES,
    "ShareService::create",
    AuthClass::SessionOnly,
    BodyClass::Small,
)
.summary("Create a share link for a file; the token is in this answer only")
.params(CREATE_PARAMS)
.errors(CREATE_ERRORS)
.request(wire::CREATE_REQUEST)
.response(wire::CREATED_SHARE)
.success(201);

const LIST: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    SHARES,
    "ShareService::list",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("The caller's own links, without tokens")
.params(LIST_PARAMS)
.errors(LIST_ERRORS)
.response(wire::SHARE_LIST);

const REVOKE: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    SHARE,
    "ShareService::revoke",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("Revoke one of the caller's links")
.params(SHARE_ID)
.errors(REVOKE_ERRORS)
.success(204);

const OPEN: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    PUBLIC_SHARE,
    "PublicShareService::open",
    AuthClass::Public,
    BodyClass::None,
)
.summary("Name, size and type of a link's file; 401 password_required before unlock")
.params(TOKEN)
.errors(VISITOR_ERRORS)
.response(wire::SHARE_OPEN);

const VERIFY: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    PASSWORD,
    "PublicShareService::verify_password",
    AuthClass::Public,
    BodyClass::Auth,
)
.summary("Check a link's password; 204 sets the grant as an HttpOnly cookie scoped to the link")
.params(TOKEN)
.errors(VISITOR_ERRORS)
.request(wire::PASSWORD_REQUEST)
.success(204);

const fn content(method: HttpMethod) -> RouteSpec {
    RouteSpec::new(
        method,
        CONTENT,
        "PublicShareService::authorize_content",
        AuthClass::ShareGrant,
        BodyClass::None,
    )
    .summary("The bytes of a link's file, through 03; Range allowed")
    .params(TOKEN)
    .errors(CONTENT_ERRORS)
    .download()
}

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(CREATE, |context, filter| {
            on(filter, owner::create).with_state(services(context))
        }),
        RouteDef::new(LIST, |context, filter| {
            on(filter, owner::list).with_state(services(context))
        }),
        RouteDef::new(REVOKE, |context, filter| {
            on(filter, owner::revoke).with_state(services(context))
        }),
        RouteDef::new(OPEN, |context, filter| {
            on(filter, visitor::open).with_state(services(context))
        }),
        RouteDef::new(VERIFY, |context, filter| {
            on(filter, visitor::verify_password).with_state(services(context))
        }),
        RouteDef::new(content(HttpMethod::Get), |context, filter| {
            on(filter, visitor::content).with_state(services(context))
        }),
        RouteDef::new(content(HttpMethod::Head), |context, filter| {
            on(filter, visitor::content).with_state(services(context))
        }),
    ]
}
