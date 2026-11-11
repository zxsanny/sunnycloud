//! The Admin operations of `api_v1_surface.md` (07 section 4 and 01 section 3): users, quotas, health and
//! invites, all for a signed-in admin. A foreign or unknown target is the port's `NotFound`, the last admin its
//! `LastAdmin` (409).

mod invites;
mod users;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const USERS: &str = "/api/v1/admin/users";
const USER: &str = "/api/v1/admin/users/{id}";
const USER_QUOTA: &str = "/api/v1/admin/users/{id}/quota";
const HEALTH: &str = "/api/v1/admin/health";
const INVITES: &str = "/api/v1/admin/invites";
const INVITE: &str = "/api/v1/admin/invites/{id}";

const ID: &[Param] = &[Param::id(ParamLocation::Path, "id", ParamType::Uuid)];
const LIST_PARAMS: &[Param] = &[
    Param::query("offset", ParamType::Integer),
    Param::query("limit", ParamType::Integer),
];

const LIST_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest];
const UPDATE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::LastAdmin,
    ErrorCode::NotFound,
];
const QUOTA_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];
const REVOKE_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest];

const LIST_USERS: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    USERS,
    "AdminService::list_users",
    AuthClass::Admin,
    BodyClass::None,
)
.summary("The accounts of the installation with their role, state and usage")
.params(LIST_PARAMS)
.errors(LIST_ERRORS)
.response(wire::USER_LIST);

const UPDATE_USER: RouteSpec = RouteSpec::new(
    HttpMethod::Patch,
    USER,
    "AdminService::update_user",
    AuthClass::Admin,
    BodyClass::Small,
)
.summary("Change an account's role or state; the last admin cannot be demoted or disabled")
.params(ID)
.errors(UPDATE_ERRORS)
.request(wire::UPDATE_REQUEST)
.success(204);

const SET_QUOTA: RouteSpec = RouteSpec::new(
    HttpMethod::Put,
    USER_QUOTA,
    "AdminService::set_quota",
    AuthClass::Admin,
    BodyClass::Small,
)
.summary("Set an account's storage limit in bytes, or restore the installation default with null")
.params(ID)
.errors(QUOTA_ERRORS)
.request(wire::QUOTA_REQUEST)
.success(204);

const HEALTH_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    HEALTH,
    "AdminService::health",
    AuthClass::Admin,
    BodyClass::None,
)
.summary("The health items of the installation's components")
.response(wire::HEALTH);

const CREATE_INVITE: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    INVITES,
    "InviteService::create_invite",
    AuthClass::Admin,
    BodyClass::Small,
)
.summary("Create a member invite; the token is in this answer only")
.errors(LIST_ERRORS)
.request(wire::INVITE_REQUEST)
.response(wire::CREATED_INVITE);

const LIST_INVITES: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    INVITES,
    "InviteService::list",
    AuthClass::Admin,
    BodyClass::None,
)
.summary("The invites, without tokens")
.response(wire::INVITE_LIST);

const REVOKE_INVITE: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    INVITE,
    "InviteService::revoke",
    AuthClass::Admin,
    BodyClass::None,
)
.summary("Revoke an invite")
.params(ID)
.errors(REVOKE_ERRORS)
.success(204);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(LIST_USERS, |context, filter| {
            on(filter, users::list).with_state(services(context))
        }),
        RouteDef::new(UPDATE_USER, |context, filter| {
            on(filter, users::update).with_state(services(context))
        }),
        RouteDef::new(SET_QUOTA, |context, filter| {
            on(filter, users::set_quota).with_state(services(context))
        }),
        RouteDef::new(HEALTH_SPEC, |context, filter| {
            on(filter, users::health).with_state(services(context))
        }),
        RouteDef::new(CREATE_INVITE, |context, filter| {
            on(filter, invites::create).with_state(services(context))
        }),
        RouteDef::new(LIST_INVITES, |context, filter| {
            on(filter, invites::list).with_state(services(context))
        }),
        RouteDef::new(REVOKE_INVITE, |context, filter| {
            on(filter, invites::revoke).with_state(services(context))
        }),
    ]
}
