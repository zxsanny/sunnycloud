//! The owner routes of 06 section 3: create, list and revoke. Session-only auth has already run, so the
//! account is the principal's and a device token never gets here (AC-57).

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use uuid::Uuid;

use super::wire;
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, User, query_value, refused, uuid_param,
};

const NODE_FILTER: &str = "node_id";

pub(super) async fn create(State(services): State<Services>, user: User, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::create_request(&body).ok_or_else(|| malformed(user.request_id))?;
    let created = services
        .shares
        .create(
            user.account,
            request.node_id,
            request.expires_in,
            request.password.as_deref(),
        )
        .await
        .map_err(refused(user.request_id))?;
    Ok((StatusCode::CREATED, Json(wire::created_share(&created))).into_response())
}

pub(super) async fn list(State(services): State<Services>, user: User, uri: Uri) -> Answer {
    let node = match query_value(&uri, NODE_FILTER) {
        Some(text) => Some(
            Uuid::parse_str(text)
                .map_err(|_| ApiProblem::validation(NODE_FILTER, "not_a_uuid", user.request_id))?,
        ),
        None => None,
    };
    let shares = services
        .shares
        .list(user.account, node)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::share_list(&shares)).into_response())
}

pub(super) async fn revoke(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let share = uuid_param(&params, "id", user.request_id)?;
    services
        .shares
        .revoke(user.account, share)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
