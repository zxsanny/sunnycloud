//! The eight worker operations of 08 section 5, for the internal listener only (`api_v1_surface.md`, Worker
//! rows). Each calls one `WorkerJobs` method; the worker bearer is checked by 08's `WorkerAuthenticator` in
//! front of every one of them, so a user or device credential never reaches a handler (AC-79).

mod caller;
mod handlers;
mod wire;

use std::sync::Arc;

use axum::routing::{MethodFilter, MethodRouter, on};
use sc_error::ErrorCode;

use super::RouteContext;
use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::spec::{Param, ParamLocation, ParamType, RouteDef, RouteSpec};
use caller::{WORKER_ID_HEADER, Worker};
use handlers::{
    claim, complete_skipped, extend_lease, fail, open_input, put_output, record_status, release,
};

const CLAIMS: &str = "/api/v1/worker-jobs/claims";
const LEASE: &str = "/api/v1/worker-jobs/{job_id}/lease";
const INPUT: &str = "/api/v1/worker-jobs/{job_id}/input";
const OUTPUT: &str = "/api/v1/worker-jobs/{job_id}/outputs/{kind}";
const RESULT: &str = "/api/v1/worker-jobs/{job_id}/result";
const FAILURE: &str = "/api/v1/worker-jobs/{job_id}/failure";
const STATUS: &str = "/api/v1/worker-jobs/status";

const fn header(name: &'static str, ty: ParamType) -> Param {
    Param {
        name,
        location: ParamLocation::Header,
        ty,
        id: false,
        required: false,
    }
}

const WORKER_ID: Param = header(WORKER_ID_HEADER, ParamType::String).required();
const JOB_ID: Param = Param::id(ParamLocation::Path, "job_id", ParamType::Uuid);

const NO_JOB: &[Param] = &[WORKER_ID];
const JOB: &[Param] = &[WORKER_ID, JOB_ID];
const OUTPUT_PARAMS: &[Param] = &[
    WORKER_ID,
    JOB_ID,
    Param::path("kind", ParamType::String),
    header("x-width", ParamType::Integer),
    header("x-height", ParamType::Integer),
    header("x-duration-ms", ParamType::Integer),
];

const LEASE_LOST: &[ErrorCode] = &[ErrorCode::LeaseLost];
const OUTPUT_ERRORS: &[ErrorCode] = &[
    ErrorCode::LeaseLost,
    ErrorCode::RevisionObsolete,
    ErrorCode::OutputTooLarge,
    ErrorCode::OutputTypeRejected,
    ErrorCode::OutputKindRejected,
    ErrorCode::LengthRequired,
];
const INPUT_ERRORS: &[ErrorCode] = &[ErrorCode::LeaseLost, ErrorCode::RevisionObsolete];

const fn worker(
    method: HttpMethod,
    path: &'static str,
    owner: &'static str,
    body: BodyClass,
) -> RouteSpec {
    RouteSpec::new(method, path, owner, AuthClass::Worker, body).internal()
}

const CLAIM: RouteSpec = worker(
    HttpMethod::Post,
    CLAIMS,
    "WorkerJobs::claim",
    BodyClass::Small,
)
.summary("Claim the next due job of a lane (204 when none is due)")
.params(NO_JOB)
.request(wire::CLAIM_REQUEST)
.response(wire::CLAIMED_JOB);

const EXTEND_LEASE: RouteSpec = worker(
    HttpMethod::Put,
    LEASE,
    "WorkerJobs::extend_lease",
    BodyClass::Small,
)
.summary("Heartbeat: extend the lease of a running job")
.params(JOB)
.errors(LEASE_LOST)
.response(wire::LEASE_EXTENDED);

const RELEASE: RouteSpec = worker(
    HttpMethod::Delete,
    LEASE,
    "WorkerJobs::release",
    BodyClass::None,
)
.summary("Release a job on graceful shutdown")
.params(JOB)
.errors(LEASE_LOST)
.success(204);

const OPEN_INPUT: RouteSpec = worker(
    HttpMethod::Get,
    INPUT,
    "WorkerJobs::open_input",
    BodyClass::None,
)
.summary("The decrypted original of the job's revision, whole, with its exact length")
.params(JOB)
.errors(INPUT_ERRORS)
.download();

const ACCEPT_OUTPUT: RouteSpec = worker(
    HttpMethod::Put,
    OUTPUT,
    "WorkerJobs::accept_output",
    BodyClass::WorkerOutput,
)
.summary("One derived output of the job (204 accepted, 200 already present)")
.params(OUTPUT_PARAMS)
.errors(OUTPUT_ERRORS)
.success(204);

const COMPLETE_SKIPPED: RouteSpec = worker(
    HttpMethod::Put,
    RESULT,
    "WorkerJobs::complete_skipped",
    BodyClass::Small,
)
.summary("Close the job without output")
.params(JOB)
.errors(LEASE_LOST)
.request(wire::RESULT_REQUEST)
.success(204);

const FAIL: RouteSpec = worker(
    HttpMethod::Post,
    FAILURE,
    "WorkerJobs::fail",
    BodyClass::Small,
)
.summary("Report a failed attempt")
.params(JOB)
.errors(LEASE_LOST)
.request(wire::FAILURE_REQUEST)
.response(wire::FAILURE_RESULT);

const RECORD_STATUS: RouteSpec = worker(
    HttpMethod::Put,
    STATUS,
    "WorkerJobs::record_status",
    BodyClass::Small,
)
.summary("The supervisor's decoder health and image age")
.params(NO_JOB)
.request(wire::STATUS_REQUEST)
.success(204);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(CLAIM, |context, filter| {
            mounted(context, filter, |filter| on(filter, claim))
        }),
        RouteDef::new(EXTEND_LEASE, |context, filter| {
            mounted(context, filter, |filter| on(filter, extend_lease))
        }),
        RouteDef::new(RELEASE, |context, filter| {
            mounted(context, filter, |filter| on(filter, release))
        }),
        RouteDef::new(OPEN_INPUT, |context, filter| {
            mounted(context, filter, |filter| on(filter, open_input))
        }),
        RouteDef::new(ACCEPT_OUTPUT, |context, filter| {
            mounted(context, filter, |filter| on(filter, put_output))
        }),
        RouteDef::new(COMPLETE_SKIPPED, |context, filter| {
            mounted(context, filter, |filter| on(filter, complete_skipped))
        }),
        RouteDef::new(FAIL, |context, filter| {
            mounted(context, filter, |filter| on(filter, fail))
        }),
        RouteDef::new(RECORD_STATUS, |context, filter| {
            mounted(context, filter, |filter| on(filter, record_status))
        }),
    ]
}

fn mounted(
    context: &RouteContext,
    filter: MethodFilter,
    route: impl FnOnce(MethodFilter) -> MethodRouter<Arc<Worker>>,
) -> MethodRouter {
    route(filter).with_state(Arc::new(Worker {
        jobs: Arc::clone(&context.services.worker_jobs),
        clock: Arc::clone(&context.services.clock),
    }))
}
