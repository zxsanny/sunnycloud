//! The invite handlers: bind, one `InviteService` call for the signed-in admin, map (01 section 3). The plaintext
//! token is in the create answer only.

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::wire;
use crate::json_body::{self, malformed};
use crate::registry::request::{Answer, BODY_LIMIT, Services, User, refused, uuid_param};

pub(super) async fn create(State(services): State<Services>, user: User, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let note = wire::invite_request(&body).ok_or_else(|| malformed(user.request_id))?;
    let token = services
        .invites
        .create_invite(user.account, &note)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::created_invite(&token)).into_response())
}

pub(super) async fn list(State(services): State<Services>, user: User) -> Answer {
    let invites = services
        .invites
        .list(user.account)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::invite_list(&invites)).into_response())
}

pub(super) async fn revoke(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let invite = uuid_param(&params, "id", user.request_id)?;
    services
        .invites
        .revoke(user.account, invite)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
