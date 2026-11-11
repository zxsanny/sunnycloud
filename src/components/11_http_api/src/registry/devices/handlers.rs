//! One handler per device operation: bind, one `DeviceGrantService` call, map (01 section 3).
//!
//! The four RFC 8628 poll refusals are not in the `sc_error` table, so `poll` answers them itself with distinct
//! machine-readable codes; every other refusal is the port's mapped answer.

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use contracts::DeviceGrantError;
use sc_error::CorrelationId;

use super::wire;
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{
    Anonymous, Answer, BODY_LIMIT, Services, User, refused, uuid_param,
};

/// The daemon starts a grant with its own address as the requester's, so the approver can compare it.
pub(super) async fn request(
    State(services): State<Services>,
    caller: Anonymous,
    body: Body,
) -> Answer {
    let request_id = caller.request_id;
    let body = json_body::read(body, BODY_LIMIT, request_id).await?;
    let (name, platform) = wire::grant_request(&body).ok_or_else(|| malformed(request_id))?;
    let grant = services
        .device_grants
        .request(&name, &platform, caller.client_ip)
        .await
        .map_err(refused(request_id))?;
    Ok(Json(wire::authorization(&grant)).into_response())
}

/// The daemon polls with its device code; the token is in the answer that finally carries it, once.
pub(super) async fn poll(
    State(services): State<Services>,
    caller: Anonymous,
    body: Body,
) -> Answer {
    let request_id = caller.request_id;
    let body = json_body::read(body, BODY_LIMIT, request_id).await?;
    let code = wire::poll_request(&body).ok_or_else(|| malformed(request_id))?;
    let token = services
        .device_grants
        .poll(&code)
        .await
        .map_err(|error| poll_refusal(error, request_id))?;
    Ok(Json(wire::device_token(&token)).into_response())
}

fn poll_refusal(error: DeviceGrantError, request_id: CorrelationId) -> ApiProblem {
    match error {
        DeviceGrantError::AuthorizationPending => {
            ApiProblem::device_poll("authorization_pending", "Authorization pending", request_id)
        }
        DeviceGrantError::SlowDown => ApiProblem::device_poll("slow_down", "Slow down", request_id),
        DeviceGrantError::AccessDenied => {
            ApiProblem::device_poll("access_denied", "Access denied", request_id)
        }
        DeviceGrantError::ExpiredToken => {
            ApiProblem::device_poll("expired_token", "Expired token", request_id)
        }
        other => ApiProblem::from_port(other, request_id),
    }
}

/// A session approves or denies a grant by the id the approval view gave it; 01 refuses a device principal.
pub(super) async fn decide(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    body: Body,
) -> Answer {
    let grant = uuid_param(&params, "id", user.request_id)?;
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let decision = wire::decision(&body).ok_or_else(|| malformed(user.request_id))?;
    services
        .device_grants
        .decide(&user.principal, grant, decision)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) async fn list(State(services): State<Services>, user: User) -> Answer {
    let devices = services
        .device_grants
        .list_devices(user.account)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::device_list(&devices)).into_response())
}

pub(super) async fn revoke(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let device = uuid_param(&params, "id", user.request_id)?;
    services
        .device_grants
        .revoke_device(user.account, device)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// A device token removes its own row: the port gets the device principal, not an id from the request.
pub(super) async fn revoke_self(State(services): State<Services>, user: User) -> Answer {
    services
        .device_grants
        .revoke_self(&user.principal)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
