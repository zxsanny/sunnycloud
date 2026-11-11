//! The Sync operations of `api_v1_surface.md` (05 section 3): the change feed and its head. Session or device
//! auth; the answers carry names, so they are `no-store` (the default of the header layer).

mod handlers;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamType, RouteDef, RouteSpec};

const CHANGES: &str = "/api/v1/changes";
const HEAD: &str = "/api/v1/changes/head";

const CHANGES_PARAMS: &[Param] = &[
    Param::query("cursor", ParamType::String).required(),
    Param::query("wait", ParamType::Integer),
    Param::query("limit", ParamType::Integer),
];

const CHANGES_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::InvalidCursor,
    ErrorCode::ResyncRequired,
    ErrorCode::RateLimited,
];

/// A held read: its clock is the `wait` plus a margin, not `request_timeout`.
const READ: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    CHANGES,
    "ChangeFeed::read",
    AuthClass::SessionOrDevice,
    BodyClass::None,
)
.summary("Nodes changed after a cursor, holding up to `wait` seconds when nothing is newer")
.params(CHANGES_PARAMS)
.errors(CHANGES_ERRORS)
.response(wire::CHANGE_PAGE)
.long_poll();

const HEAD_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    HEAD,
    "ChangeFeed::head",
    AuthClass::SessionOrDevice,
    BodyClass::None,
)
.summary("The current cursor of the account, for a new device to start from")
.response(wire::CHANGES_HEAD);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(READ, |context, filter| {
            on(filter, handlers::changes).with_state(services(context))
        }),
        RouteDef::new(HEAD_SPEC, |context, filter| {
            on(filter, handlers::head).with_state(services(context))
        }),
    ]
}
