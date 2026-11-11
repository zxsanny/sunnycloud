//! The Sessions operations of `api_v1_surface.md` (01 section 3): the caller's own sessions, listed, revoked by
//! id, or the current one revoked (logout). Session-only auth has already run, so the account is the principal's.

mod handlers;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const SESSIONS: &str = "/api/v1/sessions";
const SESSION: &str = "/api/v1/sessions/{id}";
const CURRENT: &str = "/api/v1/sessions/current";

const SESSION_ID: &[Param] = &[Param::id(ParamLocation::Path, "id", ParamType::Uuid)];
const REVOKE_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::Unauthenticated];

const LIST: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    SESSIONS,
    "SessionService::list",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("The caller's sessions")
.response(wire::SESSION_LIST);

const REVOKE: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    SESSION,
    "SessionService::revoke",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("Sign out one of the caller's sessions")
.params(SESSION_ID)
.errors(REVOKE_ERRORS)
.success(204);

const REVOKE_CURRENT: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    CURRENT,
    "SessionService::revoke",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("Sign out: revoke the session that made this request")
.success(204);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(LIST, |context, filter| {
            on(filter, handlers::list).with_state(services(context))
        }),
        RouteDef::new(REVOKE, |context, filter| {
            on(filter, handlers::revoke).with_state(services(context))
        }),
        RouteDef::new(REVOKE_CURRENT, |context, filter| {
            on(filter, handlers::revoke_current).with_state(services(context))
        }),
    ]
}
