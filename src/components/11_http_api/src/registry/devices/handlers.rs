//! One handler per device operation: bind, one `DeviceGrantService` call, map (01 section 3).
//!
//! The four RFC 8628 poll refusals are not in the `sc_error` table, so `poll` answers them itself with distinct
//! machine-readable codes; every other refusal is the port's mapped answer.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Extension, Json};
use contracts::DeviceGrantError;
use sc_error::CorrelationId;
use sc_ids::UserCode;

use super::wire;
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{
    Anonymous, Answer, BODY_LIMIT, Services, User, query_pairs, refused, uuid_param,
};

const USER_CODE_PARAM: &str = "user_code";

/// The web app's device approval page: `<public_url>/device` (AZ-4231, owner decision 2026-11-12). It is the
/// grant's `verification_uri`, so the daemon opens the browser on the configured origin, never a request's `Host`.
#[derive(Clone)]
pub(super) struct ApprovalPage(Arc<str>);

impl ApprovalPage {
    /// `public_url` has at most one trailing slash: the CSRF guard refuses any other form at startup.
    pub(super) fn new(public_url: &str) -> ApprovalPage {
        let origin = public_url.strip_suffix('/').unwrap_or(public_url);
        ApprovalPage(Arc::from(format!("{origin}/device")))
    }
}

/// The daemon starts a grant with its own address as the requester's, so the approver can compare it.
pub(super) async fn request(
    State(services): State<Services>,
    Extension(page): Extension<ApprovalPage>,
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
    Ok(Json(wire::authorization(&grant, &page.0)).into_response())
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

/// The approval page shows who asked: the code a person typed, with the address the session calls from. A code
/// that cannot exist (wrong length or characters) is a 400 for the field and never reaches the port.
pub(super) async fn lookup(State(services): State<Services>, user: User, uri: Uri) -> Answer {
    let typed = query_pairs(&uri)
        .ok_or_else(|| malformed(user.request_id))?
        .into_iter()
        .find(|(name, _)| name == USER_CODE_PARAM)
        .map(|(_, value)| value)
        .ok_or_else(|| ApiProblem::validation(USER_CODE_PARAM, "required", user.request_id))?;
    let code = UserCode::from_presented(&typed)
        .ok_or_else(|| ApiProblem::validation(USER_CODE_PARAM, "malformed", user.request_id))?;
    let view = services
        .device_grants
        .lookup_for_approval(&user.principal, &code, user.client_ip)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::approval(&view)).into_response())
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
