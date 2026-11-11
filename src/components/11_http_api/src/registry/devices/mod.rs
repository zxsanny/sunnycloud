//! The Devices operations of `api_v1_surface.md` (01 section 3): the RFC 8628 grant the desktop daemon starts and
//! polls, a session's approval of it, and the device list.

mod handlers;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const GRANTS: &str = "/api/v1/device-grants";
const GRANT: &str = "/api/v1/device-grants/{id}";
const TOKENS: &str = "/api/v1/device-tokens";
const DEVICES: &str = "/api/v1/devices";
const DEVICE: &str = "/api/v1/devices/{id}";
const OWN_DEVICE: &str = "/api/v1/devices/me";

const ID: &[Param] = &[Param::id(ParamLocation::Path, "id", ParamType::Uuid)];

const REQUEST_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::RateLimited];
const POLL_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];
const LOOKUP_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::RateLimited,
];
const LOOKUP_PARAMS: &[Param] = &[Param::query("user_code", ParamType::String).required()];
const DECIDE_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];
const REVOKE_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];

const REQUEST: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    GRANTS,
    "DeviceGrantService::request",
    AuthClass::Public,
    BodyClass::Auth,
)
.summary("Start a device grant: codes for the daemon and the person who approves it")
.errors(REQUEST_ERRORS)
.request(wire::GRANT_REQUEST)
.response(wire::GRANT);

/// Not-yet-approved, too-fast, denied and expired polls are 400 with the codes `authorization_pending`,
/// `slow_down`, `access_denied` and `expired_token` (`problem_details.md`).
const POLL: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    TOKENS,
    "DeviceGrantService::poll",
    AuthClass::Public,
    BodyClass::Auth,
)
.summary("Poll a device grant; the token is in the answer that carries it, once")
.errors(POLL_ERRORS)
.request(wire::POLL_REQUEST)
.response(wire::DEVICE_TOKEN);

const LOOKUP: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    GRANTS,
    "DeviceGrantService::lookup_for_approval",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("Who asked for the device grant a person's code names, and from where")
.params(LOOKUP_PARAMS)
.errors(LOOKUP_ERRORS)
.response(wire::APPROVAL);

const DECIDE: RouteSpec = RouteSpec::new(
    HttpMethod::Patch,
    GRANT,
    "DeviceGrantService::decide",
    AuthClass::SessionOnly,
    BodyClass::Small,
)
.summary("Approve or deny a device grant")
.params(ID)
.errors(DECIDE_ERRORS)
.request(wire::DECISION_REQUEST)
.success(204);

const LIST: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    DEVICES,
    "DeviceGrantService::list_devices",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("The caller's devices")
.response(wire::DEVICE_LIST);

const REVOKE: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    DEVICE,
    "DeviceGrantService::revoke_device",
    AuthClass::SessionOnly,
    BodyClass::None,
)
.summary("Revoke one of the caller's devices")
.params(ID)
.errors(REVOKE_ERRORS)
.success(204);

const REVOKE_SELF: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    OWN_DEVICE,
    "DeviceGrantService::revoke_self",
    AuthClass::DeviceOnly,
    BodyClass::None,
)
.summary("A desktop client signs itself out: its own device token is revoked")
.success(204);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(REQUEST, |context, filter| {
            on(filter, handlers::request).with_state(services(context))
        }),
        RouteDef::new(POLL, |context, filter| {
            on(filter, handlers::poll).with_state(services(context))
        }),
        RouteDef::new(LOOKUP, |context, filter| {
            on(filter, handlers::lookup).with_state(services(context))
        }),
        RouteDef::new(DECIDE, |context, filter| {
            on(filter, handlers::decide).with_state(services(context))
        }),
        RouteDef::new(LIST, |context, filter| {
            on(filter, handlers::list).with_state(services(context))
        }),
        RouteDef::new(REVOKE, |context, filter| {
            on(filter, handlers::revoke).with_state(services(context))
        }),
        RouteDef::new(REVOKE_SELF, |context, filter| {
            on(filter, handlers::revoke_self).with_state(services(context))
        }),
    ]
}
