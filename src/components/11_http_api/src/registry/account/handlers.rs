//! One handler per account operation: bind, one `AccountService` or `AccountDeletionService` call with the
//! caller's principal, map (07 section 3).

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::wire;
use crate::json_body::{self, malformed};
use crate::registry::request::{Answer, BODY_LIMIT, Services, User, refused};

pub(super) async fn view(State(services): State<Services>, user: User) -> Answer {
    let account = services
        .accounts
        .view(&user.principal)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::account(&account)).into_response())
}

pub(super) async fn quota(State(services): State<Services>, user: User) -> Answer {
    let quota = services
        .accounts
        .quota_view(&user.principal)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::quota(&quota)).into_response())
}

/// 202 with the deadline by which 07 erases the account; 07 owns the confirmation and last-admin rules.
pub(super) async fn delete(State(services): State<Services>, user: User, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let confirmation = wire::deletion_request(&body).ok_or_else(|| malformed(user.request_id))?;
    let accepted = services
        .account_deletion
        .request(&user.principal, &confirmation)
        .await
        .map_err(refused(user.request_id))?;
    Ok((StatusCode::ACCEPTED, Json(wire::deletion(&accepted))).into_response())
}
