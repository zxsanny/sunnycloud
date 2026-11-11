//! The Media operations of `api_v1_surface.md` (08 section 5): a node's thumbnail and transcode, and the batch
//! state of thumbnails. Session or device auth; the account is the principal's.
//!
//! `POST /thumbnail-states` takes content hashes, not node ids: `ThumbnailStateSource::states` takes hashes and
//! no port maps many node ids to hashes in one call (`api_v1_surface.md`, open point 6).

mod handlers;
pub(super) mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const THUMBNAIL: &str = "/api/v1/nodes/{node_id}/thumbnail";
const TRANSCODE: &str = "/api/v1/nodes/{node_id}/transcode";
const STATES: &str = "/api/v1/thumbnail-states";

const NODE: Param = Param::id(ParamLocation::Path, "node_id", ParamType::Uuid);
const THUMBNAIL_PARAMS: &[Param] = &[NODE, Param::query("size", ParamType::String)];
const TRANSCODE_PARAMS: &[Param] = &[NODE];

const CONTENT_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::ThumbnailNotReady,
    ErrorCode::ThumbnailUnavailable,
    ErrorCode::RangeNotSatisfiable,
    ErrorCode::ContentUnavailable,
];
const STATES_ERRORS: &[ErrorCode] = &[ErrorCode::BatchTooLarge];

const SESSION_OR_DEVICE: AuthClass = AuthClass::SessionOrDevice;

const fn thumbnail(method: HttpMethod) -> RouteSpec {
    RouteSpec::new(
        method,
        THUMBNAIL,
        "DerivedOpener::open_derived",
        SESSION_OR_DEVICE,
        BodyClass::None,
    )
    .summary("A node's WebP thumbnail (`size` small or preview), through 03")
    .params(THUMBNAIL_PARAMS)
    .errors(CONTENT_ERRORS)
    .download()
}

const fn transcode(method: HttpMethod) -> RouteSpec {
    RouteSpec::new(
        method,
        TRANSCODE,
        "DerivedOpener::open_derived",
        SESSION_OR_DEVICE,
        BodyClass::None,
    )
    .summary("A node's MP4 transcode, through 03; Range allowed")
    .params(TRANSCODE_PARAMS)
    .errors(CONTENT_ERRORS)
    .download()
}

const STATES_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    STATES,
    "ThumbnailStateSource::states",
    SESSION_OR_DEVICE,
    BodyClass::Query,
)
.summary("The thumbnail state of up to 1,000 content hashes of the caller's account")
.errors(STATES_ERRORS)
.request(wire::STATES_REQUEST)
.response(wire::STATES_RESPONSE)
.read_only();

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(thumbnail(HttpMethod::Get), |context, filter| {
            on(filter, handlers::thumbnail).with_state(services(context))
        }),
        RouteDef::new(thumbnail(HttpMethod::Head), |context, filter| {
            on(filter, handlers::thumbnail).with_state(services(context))
        }),
        RouteDef::new(transcode(HttpMethod::Get), |context, filter| {
            on(filter, handlers::transcode).with_state(services(context))
        }),
        RouteDef::new(transcode(HttpMethod::Head), |context, filter| {
            on(filter, handlers::transcode).with_state(services(context))
        }),
        RouteDef::new(STATES_SPEC, |context, filter| {
            on(filter, handlers::thumbnail_states).with_state(services(context))
        }),
    ]
}
