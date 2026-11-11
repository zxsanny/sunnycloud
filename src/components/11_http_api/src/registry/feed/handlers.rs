//! The two feed handlers of 05 section 3. `read` gets the principal and 01's revalidator so a held poll ends
//! when the principal is revoked; 11 adds nothing between its return and the response.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::Uri;
use axum::response::IntoResponse;
use sc_error::ErrorCode;

use super::wire;
use crate::problem::ApiProblem;
use crate::registry::request::{Answer, Services, User, query_number, query_value, refused};

const CURSOR_PARAM: &str = "cursor";

/// `cursor` is `E.S`; `wait` is whole seconds. A cursor that is not `E.S` never reaches the port: the answer is
/// the same 422 `invalid_cursor` as the port's `BadCursor`.
pub(super) async fn changes(State(services): State<Services>, user: User, uri: Uri) -> Answer {
    let text = query_value(&uri, CURSOR_PARAM)
        .ok_or_else(|| ApiProblem::validation(CURSOR_PARAM, "required", user.request_id))?;
    let cursor = wire::parse_cursor(text)
        .ok_or_else(|| ApiProblem::code(ErrorCode::InvalidCursor, user.request_id))?;
    let wait = query_number::<u64>(&uri, "wait", user.request_id)?.map(Duration::from_secs);
    let limit = query_number::<u32>(&uri, "limit", user.request_id)?;
    let page = services
        .change_feed
        .read(
            &user.principal,
            services.principal_revalidator.as_ref(),
            cursor,
            wait,
            limit,
        )
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::page(&page)).into_response())
}

pub(super) async fn head(State(services): State<Services>, user: User) -> Answer {
    let cursor = services
        .change_feed
        .head(user.account)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::head(cursor)).into_response())
}
