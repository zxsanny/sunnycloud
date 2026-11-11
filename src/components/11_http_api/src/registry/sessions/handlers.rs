//! One handler per session operation: bind, one `SessionService` call, map. A revocation by the caller is
//! reason `User`.

use axum::Json;
use axum::extract::{RawPathParams, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use contracts::RevokeReason;

use super::wire;
use crate::registry::request::{Answer, Services, User, refused, uuid_param};

pub(super) async fn list(State(services): State<Services>, user: User) -> Answer {
    let sessions = services
        .sessions
        .list(user.account)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::session_list(&sessions)).into_response())
}

pub(super) async fn revoke(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
) -> Answer {
    let session = uuid_param(&params, "id", user.request_id)?;
    services
        .sessions
        .revoke(user.account, session, RevokeReason::User)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The id is the caller's own session, from the principal: never one from the path.
pub(super) async fn revoke_current(State(services): State<Services>, user: User) -> Answer {
    services
        .sessions
        .revoke(
            user.account,
            user.principal.session_or_device_id,
            RevokeReason::User,
        )
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
