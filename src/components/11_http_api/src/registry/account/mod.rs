//! The Account operations of `api_v1_surface.md` (07 section 4): the caller's own view and quota, and the
//! deletion request. `GET /accounts/me` and its quota are session or device (07's scope; 11 section 8 says
//! session only, open point 37 of the surface contract); the deletion is session only.

mod handlers;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{RouteDef, RouteSpec};

pub(super) use wire::role;

const ME: &str = "/api/v1/accounts/me";
const QUOTA: &str = "/api/v1/accounts/me/quota";

const DELETE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::LastAdmin,
    ErrorCode::Forbidden,
];

const VIEW: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    ME,
    "AccountService::view",
    AuthClass::SessionOrDevice,
    BodyClass::None,
)
.summary("The caller's account: name, role, linked identities, quota and root folder")
.response(wire::ACCOUNT);

const DELETE: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    ME,
    "AccountDeletionService::request",
    AuthClass::SessionOnly,
    BodyClass::Small,
)
.summary("Request deletion of the caller's account; 202 with the erase deadline")
.errors(DELETE_ERRORS)
.request(wire::DELETION_REQUEST)
.response(wire::DELETION)
.success(202);

const QUOTA_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    QUOTA,
    "AccountService::quota_view",
    AuthClass::SessionOrDevice,
    BodyClass::None,
)
.summary("The caller's storage limit, usage and reservations")
.response(wire::QUOTA);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(VIEW, |context, filter| {
            on(filter, handlers::view).with_state(services(context))
        }),
        RouteDef::new(DELETE, |context, filter| {
            on(filter, handlers::delete).with_state(services(context))
        }),
        RouteDef::new(QUOTA_SPEC, |context, filter| {
            on(filter, handlers::quota).with_state(services(context))
        }),
    ]
}
