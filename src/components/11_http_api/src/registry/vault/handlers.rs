//! One handler per vault operation: bind, one `VaultService` call, map (ADR-015). Session auth has already run,
//! so the account is the principal's. 11 owns two answers the port does not: 428 when `If-Match` is missing
//! (no port call, so the stored header is unchanged) and the `ETag` response header.

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::header::ETAG;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use contracts::VaultService;
use sc_error::{CorrelationId, ErrorCode};

use super::wire;
use crate::json_body;
use crate::problem::ApiProblem;
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, User, missing_layer, refused, uuid_param,
};

/// The port in 11's optional slot. The routes are mounted only when 12 filled it, so an empty slot here is a
/// wiring fault, answered as an opaque 500.
fn port(services: &Services, request_id: CorrelationId) -> Result<&dyn VaultService, ApiProblem> {
    services
        .vault
        .as_deref()
        .ok_or_else(|| missing_layer("vault service", request_id))
}

pub(super) async fn create(State(services): State<Services>, user: User, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::create_request(&body, user.request_id)?;
    let created = port(&services, user.request_id)?
        .create_vault(user.account, &request.header, request.root_node_id)
        .await
        .map_err(refused(user.request_id))?;
    Ok((
        StatusCode::CREATED,
        [(ETAG, wire::etag_value(&created.etag))],
        Json(wire::created(created.vault_id)),
    )
        .into_response())
}

pub(super) async fn get(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let vault = uuid_param(&params, "id", user.request_id)?;
    let found = port(&services, user.request_id)?
        .get_header(user.account, vault)
        .await
        .map_err(refused(user.request_id))?;
    Ok((
        [(ETAG, wire::etag_value(&found.etag))],
        Json(wire::vault(vault, &found)),
    )
        .into_response())
}

pub(super) async fn replace(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    headers: HeaderMap,
    body: Body,
) -> Answer {
    let vault = uuid_param(&params, "id", user.request_id)?;
    let if_match = wire::if_match(&headers)
        .ok_or_else(|| ApiProblem::code(ErrorCode::PreconditionRequired, user.request_id))?;
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let header = wire::replace_request(&body, user.request_id)?;
    let etag = port(&services, user.request_id)?
        .replace_header(user.account, vault, &header, &if_match)
        .await
        .map_err(refused(user.request_id))?;
    Ok(([(ETAG, wire::etag_value(&etag))]).into_response())
}

/// The request body is never read: whatever a client sends with a report is neither stored nor logged.
pub(super) async fn report_unlock_failure(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let vault = uuid_param(&params, "id", user.request_id)?;
    port(&services, user.request_id)?
        .report_unlock_failure(user.account, vault)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
