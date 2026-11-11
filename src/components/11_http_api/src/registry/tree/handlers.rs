//! One handler per tree operation: bind, call the one `TreeService` or `TreeReader` method, map the result
//! (02 section 3). The account is the principal's; a node or revision of another account is the port's 404.

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use contracts::{NodeSelector, Page};
use sc_error::{CorrelationId, DomainError};
use sc_ids::FileHash;

use super::wire::{self, DEFAULT_PAGE_LIMIT};
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::media::wire::{hash_texts, parse_hash};
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, User, query_number, query_text, query_uuid, refused, uuid_param,
};

const ID_PARAM: &str = "id";
const HASHES_FIELD: &str = "content_hashes";

pub(super) async fn root(State(services): State<Services>, user: User) -> Answer {
    let node = services
        .tree_reader
        .get_node(user.account, NodeSelector::Root)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::node_view(&node)).into_response())
}

pub(super) async fn get_node(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let id = uuid_param(&params, ID_PARAM, user.request_id)?;
    let node = services
        .tree_reader
        .get_node(user.account, NodeSelector::Id(id))
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::node_view(&node)).into_response())
}

pub(super) async fn create_folder(
    State(services): State<Services>,
    user: User,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::create_folder(&body).ok_or_else(|| malformed(user.request_id))?;
    let node = services
        .tree
        .create_folder(user.account, request.parent_id, &request.name)
        .await
        .map_err(refused(user.request_id))?;
    Ok((StatusCode::CREATED, Json(wire::node_view(&node))).into_response())
}

pub(super) async fn update_node(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    body: Body,
) -> Answer {
    let id = uuid_param(&params, ID_PARAM, user.request_id)?;
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::update_node(&body).ok_or_else(|| malformed(user.request_id))?;
    let node = services
        .tree
        .update_node(user.account, id, request.name.as_deref(), request.parent_id)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::node_view(&node)).into_response())
}

pub(super) async fn delete_node(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    uri: Uri,
) -> Answer {
    let id = uuid_param(&params, ID_PARAM, user.request_id)?;
    let base_revision = query_uuid(&uri, "base_revision", user.request_id)?;
    services
        .tree
        .delete_node(user.account, id, base_revision)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) async fn list_children(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    uri: Uri,
) -> Answer {
    let folder = uuid_param(&params, ID_PARAM, user.request_id)?;
    let after = query_text(&uri, "after", user.request_id)?;
    let limit = query_number(&uri, "limit", user.request_id)?.unwrap_or(DEFAULT_PAGE_LIMIT);
    let page = services
        .tree_reader
        .list_children(user.account, folder, after.as_deref(), limit)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::listing(&page)).into_response())
}

pub(super) async fn get_revision(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let id = uuid_param(&params, ID_PARAM, user.request_id)?;
    let revision = services
        .tree_reader
        .get_revision(user.account, id)
        .await
        .map_err(refused(user.request_id))?;
    let view = wire::revision_view(&revision).map_err(corrupt(user.request_id))?;
    Ok(Json(view).into_response())
}

pub(super) async fn find_revisions(
    State(services): State<Services>,
    user: User,
    uri: Uri,
) -> Answer {
    let device = query_text(&uri, "client_device", user.request_id)?;
    let page = Page {
        offset: query_number(&uri, "offset", user.request_id)?.unwrap_or(0),
        limit: query_number(&uri, "limit", user.request_id)?.unwrap_or(DEFAULT_PAGE_LIMIT),
    };
    let found = services
        .tree
        .find_revisions(user.account, device.as_deref(), page)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::revision_refs(&found)).into_response())
}

/// One call for up to 1,000 hashes; 02 refuses more with `BatchTooLarge`.
pub(super) async fn lookup_revisions(
    State(services): State<Services>,
    user: User,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let texts = hash_texts(&body).ok_or_else(|| malformed(user.request_id))?;
    let hashes: Vec<FileHash> = texts
        .into_iter()
        .map(parse_hash)
        .collect::<Option<_>>()
        .ok_or_else(|| ApiProblem::validation(HASHES_FIELD, "not_a_hash", user.request_id))?;
    let found = services
        .tree_reader
        .lookup_revisions(user.account, &hashes)
        .await
        .map_err(refused(user.request_id))?;
    let matches = wire::revision_matches(&found).map_err(corrupt(user.request_id))?;
    Ok(Json(matches).into_response())
}

/// A port handed back `client_meta` text that is not JSON: its fault, answered as an opaque 500 and logged.
fn corrupt(request_id: CorrelationId) -> impl FnOnce(serde_json::Error) -> ApiProblem {
    move |failure| ApiProblem::new(DomainError::unexpected(failure, request_id), request_id)
}
