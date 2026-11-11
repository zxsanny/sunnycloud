//! One handler per worker operation: bind, call the one `WorkerJobs` method, map the result (08 section 5).
//! Auth has already run, so nothing here sees an unauthenticated caller.

use std::str::FromStr;
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use contracts::{AcceptOutcome, ByteStream, DerivedKind, OutputMeta, StreamError, WorkerJobError};
use http_body_util::BodyExt;
use sc_error::ErrorCode;

use super::caller::{Caller, JobPath, Worker};
use super::wire::{
    claim_lane, claimed_job, fail_result, failure, lease_extended, skip_outcome, worker_status,
};
use crate::content;
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{Answer, BODY_LIMIT};

pub(super) async fn claim(State(worker): State<Arc<Worker>>, caller: Caller, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, caller.request_id).await?;
    let lane = claim_lane(&body).ok_or_else(|| malformed(caller.request_id))?;
    let claimed = worker
        .jobs
        .claim(lane, &caller.worker_id)
        .await
        .map_err(refused(&caller))?;
    Ok(match claimed {
        Some(job) => Json(claimed_job(&job)).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

pub(super) async fn extend_lease(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    job: JobPath,
) -> Answer {
    let extended = worker
        .jobs
        .extend_lease(job.job_id, &caller.worker_id)
        .await
        .map_err(refused(&caller))?;
    Ok(Json(lease_extended(extended.lease_ms)).into_response())
}

pub(super) async fn release(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    job: JobPath,
) -> Answer {
    worker
        .jobs
        .release(job.job_id, &caller.worker_id)
        .await
        .map_err(refused(&caller))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The whole decrypted input, never ranged: always 200 with the exact length the port reports.
pub(super) async fn open_input(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    job: JobPath,
) -> Answer {
    let input = worker
        .jobs
        .open_input(job.job_id, &caller.worker_id)
        .await
        .map_err(refused(&caller))?;
    let mut response = Response::new(content::stream(input.body, caller.request_id));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from(input.length),
    );
    Ok(response)
}

pub(super) async fn put_output(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    job: JobPath,
    request: Request,
) -> Answer {
    let kind = output_kind(job.kind.as_deref())
        .ok_or_else(|| ApiProblem::code(ErrorCode::OutputKindRejected, caller.request_id))?;
    let (parts, body) = request.into_parts();
    let length = number::<u64>(&parts.headers, "content-length", &caller)?
        .ok_or_else(|| ApiProblem::code(ErrorCode::LengthRequired, caller.request_id))?;
    let meta = output_meta(&parts.headers, &caller)?;
    let outcome = worker
        .jobs
        .accept_output(
            job.job_id,
            &caller.worker_id,
            kind,
            meta,
            byte_stream(body),
            length,
        )
        .await
        .map_err(refused(&caller))?;
    Ok(match outcome {
        AcceptOutcome::Accepted => StatusCode::NO_CONTENT,
        AcceptOutcome::AlreadyPresent => StatusCode::OK,
    }
    .into_response())
}

pub(super) async fn complete_skipped(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    job: JobPath,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, caller.request_id).await?;
    let outcome = skip_outcome(&body).ok_or_else(|| malformed(caller.request_id))?;
    worker
        .jobs
        .complete_skipped(job.job_id, &caller.worker_id, outcome)
        .await
        .map_err(refused(&caller))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) async fn fail(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    job: JobPath,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, caller.request_id).await?;
    let (code, disposition) = failure(&body).ok_or_else(|| malformed(caller.request_id))?;
    let result = worker
        .jobs
        .fail(job.job_id, &caller.worker_id, code, disposition)
        .await
        .map_err(refused(&caller))?;
    Ok(Json(fail_result(result)).into_response())
}

pub(super) async fn record_status(
    State(worker): State<Arc<Worker>>,
    caller: Caller,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, caller.request_id).await?;
    let status = worker_status(&body).ok_or_else(|| malformed(caller.request_id))?;
    worker
        .jobs
        .record_status(&caller.worker_id, status)
        .await
        .map_err(refused(&caller))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn refused(caller: &Caller) -> impl FnOnce(WorkerJobError) -> ApiProblem {
    let request_id = caller.request_id;
    move |error| ApiProblem::from_port(error, request_id)
}

/// The kind of a worker output route; any other value is a kind no job produces.
fn output_kind(kind: Option<&str>) -> Option<DerivedKind> {
    match kind? {
        "thumb" => Some(DerivedKind::Thumb),
        "preview" => Some(DerivedKind::Preview),
        "transcode" => Some(DerivedKind::Transcode),
        _ => None,
    }
}

fn output_meta(headers: &HeaderMap, caller: &Caller) -> Result<OutputMeta, ApiProblem> {
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiProblem::validation("content-type", "required", caller.request_id))?;
    Ok(OutputMeta {
        media_type: media_type.to_owned(),
        width: number(headers, "x-width", caller)?,
        height: number(headers, "x-height", caller)?,
        duration_ms: number(headers, "x-duration-ms", caller)?,
    })
}

/// A numeric header: `None` when absent, a 400 when present and not a number.
fn number<T: FromStr>(
    headers: &HeaderMap,
    name: &'static str,
    caller: &Caller,
) -> Result<Option<T>, ApiProblem> {
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok()
        .and_then(|text| text.parse().ok())
        .map(Some)
        .ok_or_else(|| ApiProblem::validation(name, "malformed", caller.request_id))
}

fn byte_stream(body: Body) -> ByteStream {
    Box::pin(
        body.map_err(|failure| StreamError::Source(Box::new(failure)))
            .into_data_stream(),
    )
}
