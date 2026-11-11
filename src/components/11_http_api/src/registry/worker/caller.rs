use std::sync::Arc;

use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::request::Parts;
use contracts::WorkerJobs;
use sc_clock::Clock;
use sc_error::CorrelationId;
use uuid::Uuid;

use crate::problem::ApiProblem;

pub(super) const WORKER_ID_HEADER: &str = "x-worker-id";
const JOB_PARAM: &str = "job_id";
const KIND_PARAM: &str = "kind";

/// What every worker handler holds: the one port it calls, and the clock that gives a fresh request id to a
/// router mounted without the request-id layer.
pub(super) struct Worker {
    pub(super) jobs: Arc<dyn WorkerJobs>,
    pub(super) clock: Arc<dyn Clock>,
}

pub(super) fn request_id(parts: &Parts, clock: &dyn Clock) -> CorrelationId {
    parts
        .extensions
        .get::<CorrelationId>()
        .copied()
        .unwrap_or_else(|| CorrelationId::new(sc_ids::new_id(clock)))
}

/// The authenticated worker's call: its `X-Worker-Id`, passed through to the port, and the request id.
pub(super) struct Caller {
    pub(super) worker_id: String,
    pub(super) request_id: CorrelationId,
}

impl FromRequestParts<Arc<Worker>> for Caller {
    type Rejection = ApiProblem;

    async fn from_request_parts(
        parts: &mut Parts,
        worker: &Arc<Worker>,
    ) -> Result<Self, ApiProblem> {
        let request_id = request_id(parts, worker.clock.as_ref());
        let mut values = parts.headers.get_all(WORKER_ID_HEADER).iter();
        let worker_id = match (values.next(), values.next()) {
            (Some(value), None) => value.to_str().ok().filter(|id| !id.is_empty()),
            _ => None,
        };
        match worker_id {
            Some(id) => Ok(Caller {
                worker_id: id.to_owned(),
                request_id,
            }),
            None => Err(ApiProblem::validation(
                WORKER_ID_HEADER,
                "required",
                request_id,
            )),
        }
    }
}

/// The `{job_id}` of the route, and its `{kind}` when it has one.
pub(super) struct JobPath {
    pub(super) job_id: Uuid,
    pub(super) kind: Option<String>,
}

impl FromRequestParts<Arc<Worker>> for JobPath {
    type Rejection = ApiProblem;

    async fn from_request_parts(
        parts: &mut Parts,
        worker: &Arc<Worker>,
    ) -> Result<Self, ApiProblem> {
        let request_id = request_id(parts, worker.clock.as_ref());
        let params = RawPathParams::from_request_parts(parts, worker)
            .await
            .map_err(|_| ApiProblem::validation(JOB_PARAM, "required", request_id))?;
        let value = |name: &str| {
            params
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value)
        };
        let job_id = value(JOB_PARAM)
            .and_then(|text| Uuid::parse_str(text).ok())
            .ok_or_else(|| ApiProblem::validation(JOB_PARAM, "not_a_uuid", request_id))?;
        Ok(JobPath {
            job_id,
            kind: value(KIND_PARAM).map(str::to_owned),
        })
    }
}
