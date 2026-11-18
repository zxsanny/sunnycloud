//! The vault operations of `vault_http_api.md` (20 section 4, v1.2): create a vault, read and replace its opaque
//! header with `ETag` / `If-Match`, report an unlock failure. They are served only when 12 filled the vault slot
//! of `ApiServices` ([`super::served_routes`]); v1.0 and v1.1 builds have neither the routes nor their document
//! entries.

mod handlers;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const VAULTS: &str = "/api/v1/vaults";
const VAULT: &str = "/api/v1/vaults/{id}";
const HEADER: &str = "/api/v1/vaults/{id}/header";
const UNLOCK_FAILURES: &str = "/api/v1/vaults/{id}/unlock-failures";

const VAULT_ID: Param = Param::id(ParamLocation::Path, "id", ParamType::Uuid);
const IF_MATCH: Param = Param {
    name: "If-Match",
    location: ParamLocation::Header,
    ty: ParamType::String,
    id: false,
    required: true,
};

const CREATE_PARAMS: &[Param] = &[Param::id(
    ParamLocation::Body,
    "root_node_id",
    ParamType::Uuid,
)];
const VAULT_PARAMS: &[Param] = &[VAULT_ID];
const REPLACE_PARAMS: &[Param] = &[VAULT_ID, IF_MATCH];

const CREATE_ERRORS: &[ErrorCode] = &[
    ErrorCode::NotFound,
    ErrorCode::VaultExists,
    ErrorCode::VaultHeaderTooLarge,
];
const READ_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::NotFound];
const REPLACE_ERRORS: &[ErrorCode] = &[
    ErrorCode::NotFound,
    ErrorCode::PreconditionFailed,
    ErrorCode::PreconditionRequired,
    ErrorCode::VaultHeaderTooLarge,
];
const REPORT_ERRORS: &[ErrorCode] = &[ErrorCode::NotFound];

const CREATE: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    VAULTS,
    "VaultService::create_vault",
    AuthClass::SessionOnly,
    BodyClass::Small,
)
.summary("Create a vault on one of the caller's folders; 201 with the header's `ETag`")
.params(CREATE_PARAMS)
.errors(CREATE_ERRORS)
.request(wire::CREATE_REQUEST)
.response(wire::CREATED)
.success(201);

const READ: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    VAULT,
    "VaultService::get_header",
    AuthClass::Session,
    BodyClass::None,
)
.summary("The vault's opaque header (base64) and root folder, with the header's `ETag`")
.params(VAULT_PARAMS)
.errors(READ_ERRORS)
.response(wire::VAULT);

const REPLACE: RouteSpec = RouteSpec::new(
    HttpMethod::Put,
    HEADER,
    "VaultService::replace_header",
    AuthClass::SessionOnly,
    BodyClass::Small,
)
.summary("Replace the header when `If-Match` is its current `ETag` (428 without it, 412 when it differs)")
.params(REPLACE_PARAMS)
.errors(REPLACE_ERRORS)
.request(wire::REPLACE_REQUEST);

const REPORT: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    UNLOCK_FAILURES,
    "VaultService::report_unlock_failure",
    AuthClass::Session,
    BodyClass::Small,
)
.summary("Report a failed unlock; any body is ignored, never stored or logged")
.params(VAULT_PARAMS)
.errors(REPORT_ERRORS)
.success(204);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(CREATE, |context, filter| {
            on(filter, handlers::create).with_state(services(context))
        }),
        RouteDef::new(READ, |context, filter| {
            on(filter, handlers::get).with_state(services(context))
        }),
        RouteDef::new(REPLACE, |context, filter| {
            on(filter, handlers::replace).with_state(services(context))
        }),
        RouteDef::new(REPORT, |context, filter| {
            on(filter, handlers::report_unlock_failure).with_state(services(context))
        }),
    ]
}
