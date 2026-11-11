//! The Transfer operations of `api_v1_surface.md` (03 section 4): upload sessions, blocks, staged uploads, single
//! and batch commits, and the content of a node or a revision. Session or device auth; the account is the
//! principal's.

mod commits;
mod content;
mod uploads;
mod wire;

use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};

const UPLOADS: &str = "/api/v1/uploads";
const UPLOAD: &str = "/api/v1/uploads/{upload_id}";
const BLOCK: &str = "/api/v1/uploads/{upload_id}/blocks/{index}";
const STAGED: &str = "/api/v1/staged-uploads/{staged_key}";
const COMMITS: &str = "/api/v1/commits";
const BATCH: &str = "/api/v1/commits:batch";
const NODE_CONTENT: &str = "/api/v1/nodes/{node_id}/content";
const REVISION_CONTENT: &str = "/api/v1/revisions/{revision_id}/content";

const UPLOAD_ID: Param = Param::id(ParamLocation::Path, "upload_id", ParamType::Uuid);
const HASH_HEADER: Param = Param {
    name: "x-blake3",
    location: ParamLocation::Header,
    ty: ParamType::String,
    id: false,
    required: false,
};

const START_PARAMS: &[Param] = &[
    Param::id(ParamLocation::Body, "parent_id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "base_revision", ParamType::Uuid),
];
const UPLOAD_PARAMS: &[Param] = &[UPLOAD_ID];
const BLOCK_PARAMS: &[Param] = &[
    UPLOAD_ID,
    Param::path("index", ParamType::Integer),
    HASH_HEADER.required(),
];
const STAGED_PARAMS: &[Param] = &[
    Param::id(ParamLocation::Path, "staged_key", ParamType::Uuid),
    HASH_HEADER,
];
const COMMIT_PARAMS: &[Param] = &[
    Param::id(ParamLocation::Body, "upload_id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "parent_id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "base_revision", ParamType::Uuid),
];
const BATCH_PARAMS: &[Param] = &[
    Param::id(ParamLocation::Body, "staged_key", ParamType::Uuid),
    Param::id(ParamLocation::Body, "parent_id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "base_revision", ParamType::Uuid),
];
const NODE_CONTENT_PARAMS: &[Param] = &[Param::id(ParamLocation::Path, "node_id", ParamType::Uuid)];
const REVISION_CONTENT_PARAMS: &[Param] = &[Param::id(
    ParamLocation::Path,
    "revision_id",
    ParamType::Uuid,
)];

const SESSION_OR_DEVICE: AuthClass = AuthClass::SessionOrDevice;

const START_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidName,
    ErrorCode::ParentNotFound,
    ErrorCode::NotAFolder,
    ErrorCode::SizeTooLarge,
    ErrorCode::QuotaExceeded,
    ErrorCode::SessionCapExceeded,
    ErrorCode::FreeSpaceFloor,
    ErrorCode::AccountNotActive,
];
const UPLOAD_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::UploadSessionGone];
const BLOCK_ERRORS: &[ErrorCode] = &[
    ErrorCode::UploadSessionGone,
    ErrorCode::LengthRequired,
    ErrorCode::BlockLengthMismatch,
    ErrorCode::BlockHashMismatch,
    ErrorCode::BlockConflict,
    ErrorCode::Overloaded,
    ErrorCode::FreeSpaceFloor,
];
const STAGED_ERRORS: &[ErrorCode] = &[
    ErrorCode::QuotaExceeded,
    ErrorCode::StagedCapReached,
    ErrorCode::BlockHashMismatch,
    ErrorCode::BlockLengthMismatch,
    ErrorCode::LengthRequired,
    ErrorCode::FreeSpaceFloor,
    ErrorCode::Overloaded,
    ErrorCode::AccountNotActive,
];
const COMMIT_ERRORS: &[ErrorCode] = &[
    ErrorCode::NeedBlocks,
    ErrorCode::BlocklistMismatch,
    ErrorCode::UploadSessionGone,
    ErrorCode::InvalidName,
    ErrorCode::EditedSince,
    ErrorCode::ParentDeleted,
    ErrorCode::ParentNotFound,
    ErrorCode::NotAFolder,
    ErrorCode::AccountNotActive,
];
const BATCH_ERRORS: &[ErrorCode] = &[
    ErrorCode::BatchTooLarge,
    ErrorCode::CommitIdReused,
    ErrorCode::AccountNotActive,
];
const CONTENT_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::RangeNotSatisfiable,
    ErrorCode::ContentUnavailable,
];

const START: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    UPLOADS,
    "UploadService::start_upload",
    SESSION_OR_DEVICE,
    BodyClass::Small,
)
.summary("Start or resume a large upload (the same `client_key` returns the same session), or learn there is nothing to send")
.params(START_PARAMS)
.errors(START_ERRORS)
.request(wire::START_REQUEST)
.response(wire::START_RESPONSE);

const GET_UPLOAD: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    UPLOAD,
    "UploadService::get_upload",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("The blocks an upload still needs")
.params(UPLOAD_PARAMS)
.errors(UPLOAD_ERRORS)
.response(wire::UPLOAD_STATE);

const ABORT_UPLOAD: RouteSpec = RouteSpec::new(
    HttpMethod::Delete,
    UPLOAD,
    "UploadService::abort_upload",
    SESSION_OR_DEVICE,
    BodyClass::None,
)
.summary("Abort an upload and release its reservation")
.params(UPLOAD_PARAMS)
.errors(UPLOAD_ERRORS)
.success(204);

const PUT_BLOCK: RouteSpec = RouteSpec::new(
    HttpMethod::Put,
    BLOCK,
    "UploadService::put_block",
    SESSION_OR_DEVICE,
    BodyClass::Block,
)
.summary("One block of at most 2 MiB; `X-Blake3` is its hash, `Content-Length` its exact length; 204 once it is durable")
.params(BLOCK_PARAMS)
.errors(BLOCK_ERRORS)
.success(204);

const STAGE_SMALL: RouteSpec = RouteSpec::new(
    HttpMethod::Put,
    STAGED,
    "UploadService::stage_small",
    SESSION_OR_DEVICE,
    BodyClass::Staged,
)
.summary(
    "A whole small file of up to 2 MiB in one request (zero bytes allowed); idempotent by its key",
)
.params(STAGED_PARAMS)
.errors(STAGED_ERRORS)
.response(wire::STAGED);

const COMMIT: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    COMMITS,
    "CommitService::commit_upload",
    SESSION_OR_DEVICE,
    BodyClass::Commit,
)
.summary("Publish one finished upload as a revision")
.params(COMMIT_PARAMS)
.errors(COMMIT_ERRORS)
.request(wire::COMMIT_REQUEST)
.response(wire::COMMITTED);

const COMMIT_BATCH: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    BATCH,
    "CommitService::commit_batch",
    SESSION_OR_DEVICE,
    BodyClass::Batch,
)
.summary("Publish staged files in one idempotent batch; a repeated `commit_id` returns the stored result")
.params(BATCH_PARAMS)
.errors(BATCH_ERRORS)
.request(wire::BATCH_REQUEST)
.response(wire::BATCH_RESULT);

const fn node_content(method: HttpMethod) -> RouteSpec {
    RouteSpec::new(
        method,
        NODE_CONTENT,
        "DownloadService::open_revision_content",
        SESSION_OR_DEVICE,
        BodyClass::None,
    )
    .summary("The bytes of a node's current revision; Range allowed")
    .params(NODE_CONTENT_PARAMS)
    .errors(CONTENT_ERRORS)
    .download()
}

const fn revision_content(method: HttpMethod) -> RouteSpec {
    RouteSpec::new(
        method,
        REVISION_CONTENT,
        "DownloadService::open_revision_content",
        SESSION_OR_DEVICE,
        BodyClass::None,
    )
    .summary(
        "The bytes of one revision; one that is not current is served to an admin session only",
    )
    .params(REVISION_CONTENT_PARAMS)
    .errors(CONTENT_ERRORS)
    .download()
}

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(START, |context, filter| {
            on(filter, uploads::start).with_state(services(context))
        }),
        RouteDef::new(GET_UPLOAD, |context, filter| {
            on(filter, uploads::state).with_state(services(context))
        }),
        RouteDef::new(ABORT_UPLOAD, |context, filter| {
            on(filter, uploads::abort).with_state(services(context))
        }),
        RouteDef::new(PUT_BLOCK, |context, filter| {
            on(filter, uploads::put_block).with_state(services(context))
        }),
        RouteDef::new(STAGE_SMALL, |context, filter| {
            on(filter, uploads::stage_small).with_state(services(context))
        }),
        RouteDef::new(COMMIT, |context, filter| {
            on(filter, commits::commit).with_state(services(context))
        }),
        RouteDef::new(COMMIT_BATCH, |context, filter| {
            on(filter, commits::commit_batch).with_state(services(context))
        }),
        RouteDef::new(node_content(HttpMethod::Get), |context, filter| {
            on(filter, content::node_content).with_state(services(context))
        }),
        RouteDef::new(node_content(HttpMethod::Head), |context, filter| {
            on(filter, content::node_content).with_state(services(context))
        }),
        RouteDef::new(revision_content(HttpMethod::Get), |context, filter| {
            on(filter, content::revision_content).with_state(services(context))
        }),
        RouteDef::new(revision_content(HttpMethod::Head), |context, filter| {
            on(filter, content::revision_content).with_state(services(context))
        }),
    ]
}
