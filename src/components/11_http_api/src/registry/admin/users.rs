//! The admin handlers of users, quotas and health: bind, one `AdminService` call acting as the signed-in admin,
//! map (07 section 4). The target account of a path is the thing acted on; the actor is always the principal.

use axum::Json;
use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use contracts::{Actor, Page};

use super::wire;
use crate::json_body::{self, malformed};
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, User, query_number, refused, uuid_param,
};

/// `listing_default_limit` of 02, as for the other listings: `list_users` takes a number.
const DEFAULT_PAGE_LIMIT: u32 = 1000;

fn actor(user: &User) -> Actor {
    Actor::Admin(user.principal.clone())
}

pub(super) async fn list(State(services): State<Services>, user: User, uri: Uri) -> Answer {
    let offset = query_number(&uri, "offset", user.request_id)?.unwrap_or(0);
    let limit = query_number(&uri, "limit", user.request_id)?.unwrap_or(DEFAULT_PAGE_LIMIT);
    let users = services
        .admin
        .list_users(&actor(&user), Page { offset, limit })
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::user_list(&users)).into_response())
}

pub(super) async fn update(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    body: Body,
) -> Answer {
    let target = uuid_param(&params, "id", user.request_id)?;
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let (role, state) = wire::update_request(&body).ok_or_else(|| malformed(user.request_id))?;
    services
        .admin
        .update_user(&actor(&user), target, role, state)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) async fn set_quota(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    body: Body,
) -> Answer {
    let target = uuid_param(&params, "id", user.request_id)?;
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let limit = wire::quota_request(&body).ok_or_else(|| malformed(user.request_id))?;
    services
        .admin
        .set_quota(&actor(&user), target, limit)
        .await
        .map_err(refused(user.request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The report cannot fail: a contributor that failed is an item of it.
pub(super) async fn health(State(services): State<Services>, user: User) -> Answer {
    let report = services.admin.health(&actor(&user)).await;
    Ok(Json(wire::health(&report)).into_response())
}
