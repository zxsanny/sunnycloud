//! The upload routes of 03 section 4: start or resume, state, abort, the block PUT and the staged PUT. The
//! account is the principal's. A block or staged body is handed to 03 as it arrives: the handler reads none of it,
//! and answers only after 03 returns.

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use hyper::body::Body as HttpBody;
use sc_error::{CorrelationId, ErrorCode};
use sc_ids::Hash32;

use super::wire;
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, User, byte_stream, content_length, digest, refused, uuid_param,
};

const UPLOAD_PARAM: &str = "upload_id";
const INDEX_PARAM: &str = "index";
const STAGED_PARAM: &str = "staged_key";
const HASH_HEADER: &str = "x-blake3";

pub(super) async fn start(State(services): State<Services>, user: User, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::start_upload(&body).ok_or_else(|| malformed(user.request_id))?;
    let outcome = services
        .uploads
        .start_upload(user.account, request)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::start_outcome(&outcome)).into_response())
}

pub(super) async fn state(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let upload = uuid_param(&params, UPLOAD_PARAM, user.request_id)?;
    let state = services
        .uploads
        .get_upload(user.account, upload)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::upload_state(upload, &state)).into_response())
}

pub(super) async fn abort(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let upload = uuid_param(&params, UPLOAD_PARAM, user.request_id)?;
    services
        .uploads
        .abort_upload(user.account, upload)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// 204 means the block is durable: it is sent only after `put_block` returns its acknowledgement.
pub(super) async fn put_block(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    headers: HeaderMap,
    body: Body,
) -> Answer {
    let upload = uuid_param(&params, UPLOAD_PARAM, user.request_id)?;
    let index = index_param(&params, user.request_id)?;
    let hash = block_hash(&headers, user.request_id)?
        .ok_or_else(|| ApiProblem::validation(HASH_HEADER, "required", user.request_id))?;
    let length = declared_length(&headers, &body, user.request_id)?;
    services
        .uploads
        .put_block(user.account, upload, index, hash, byte_stream(body), length)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// One request, zero to 2 MiB. `X-Blake3` is absent only for a zero-length body; 03 decides what a missing
/// hash on a body that is not empty means.
pub(super) async fn stage_small(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    headers: HeaderMap,
    body: Body,
) -> Answer {
    let staged_key = uuid_param(&params, STAGED_PARAM, user.request_id)?;
    let hash = block_hash(&headers, user.request_id)?;
    let length = declared_length(&headers, &body, user.request_id)?;
    let staged = services
        .uploads
        .stage_small(user.account, staged_key, hash, byte_stream(body), length)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::staged(&staged)).into_response())
}

fn index_param(params: &RawPathParams, request_id: CorrelationId) -> Result<u32, ApiProblem> {
    params
        .iter()
        .find(|(key, _)| *key == INDEX_PARAM)
        .and_then(|(_, value)| value.parse().ok())
        .ok_or_else(|| ApiProblem::validation(INDEX_PARAM, "malformed", request_id))
}

/// The `X-Blake3` hash: 64 hex digits, or `None` when the header is absent.
fn block_hash(
    headers: &HeaderMap,
    request_id: CorrelationId,
) -> Result<Option<Hash32>, ApiProblem> {
    let Some(value) = headers.get(HASH_HEADER) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok()
        .and_then(digest)
        .map(|bytes| Some(Hash32::from_bytes(bytes)))
        .ok_or_else(|| ApiProblem::validation(HASH_HEADER, "malformed", request_id))
}

/// The length 03 is told: the declared `Content-Length`, or the exact size of a body that has no header (an empty
/// request has none, and a body of unknown length is 411).
fn declared_length(
    headers: &HeaderMap,
    body: &Body,
    request_id: CorrelationId,
) -> Result<u64, ApiProblem> {
    content_length(headers)
        .or_else(|| body.size_hint().exact())
        .ok_or_else(|| ApiProblem::code(ErrorCode::LengthRequired, request_id))
}
