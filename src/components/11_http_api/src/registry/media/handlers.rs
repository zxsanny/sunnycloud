//! The media routes of 08 section 5: a node's thumbnail or transcode, and the thumbnail states of a batch of
//! content hashes. 08 finds the derived blob and never serves; 11 passes it to 03 and answers through the
//! shared header mapper, so a thumbnail follows the same serving rules as any other content (AC-47).

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::{HeaderMap, Method, Uri};
use axum::response::IntoResponse;
use contracts::DerivedKind;
use sc_error::ErrorCode;
use sc_ids::FileHash;

use super::wire;
use crate::content::{content_response, download_options};
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, User, query_value, refused, uuid_param,
};

const NODE_PARAM: &str = "node_id";
const SIZE_PARAM: &str = "size";
const HASHES_FIELD: &str = "content_hashes";

pub(super) async fn thumbnail(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Answer {
    let kind = wire::thumbnail_kind(query_value(&uri, SIZE_PARAM))
        .ok_or_else(|| ApiProblem::validation(SIZE_PARAM, "invalid", user.request_id))?;
    derived(&services, &user, &params, kind, &method, &headers).await
}

pub(super) async fn transcode(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    method: Method,
    headers: HeaderMap,
) -> Answer {
    derived(
        &services,
        &user,
        &params,
        DerivedKind::Transcode,
        &method,
        &headers,
    )
    .await
}

/// 08 opens the derived blob of the caller's own node (`NotReady` and `NotFound` are its 404 codes), then 03
/// streams it. The caller's account scopes both, so another account's node is the same answer as a missing one.
async fn derived(
    services: &Services,
    user: &User,
    params: &RawPathParams,
    kind: DerivedKind,
    method: &Method,
    headers: &HeaderMap,
) -> Answer {
    let node = uuid_param(params, NODE_PARAM, user.request_id)?;
    let blob = services
        .derived
        .open_derived(user.account, node, kind)
        .await
        .map_err(refused(user.request_id))?;
    let opened = services
        .downloads
        .open_blob_content(
            user.account,
            blob,
            Some(wire::media_type(kind).to_owned()),
            download_options(headers),
        )
        .await;
    Ok(content_response(opened, method, user.request_id))
}

/// One call to `ThumbnailStateSource::states` for up to 1,000 content hashes of the caller's account.
pub(super) async fn thumbnail_states(
    State(services): State<Services>,
    user: User,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let texts = wire::hash_texts(&body).ok_or_else(|| malformed(user.request_id))?;
    if texts.len() > wire::MAX_HASHES {
        return Err(ApiProblem::code(ErrorCode::BatchTooLarge, user.request_id));
    }
    let hashes: Vec<FileHash> = texts
        .into_iter()
        .map(wire::parse_hash)
        .collect::<Option<_>>()
        .ok_or_else(|| ApiProblem::validation(HASHES_FIELD, "not_a_hash", user.request_id))?;
    let found = services
        .thumbnail_states
        .states(user.account, &hashes)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::states(&found)).into_response())
}
