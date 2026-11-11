//! The commit routes of 03 section 4. The device label of the principal is the one the conflict-copy name
//! carries (02, decision 15). Nothing is awaited between a port's return and the response, so the order in which
//! 02 decided the commits is the order clients see (AC-69).

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::response::IntoResponse;
use contracts::CommitActor;

use super::wire;
use crate::json_body::{self, malformed};
use crate::registry::request::{Answer, BODY_LIMIT, Services, User, refused};

fn actor(user: &User) -> CommitActor {
    CommitActor {
        account_id: user.account,
        device_label: user.principal.label.clone(),
    }
}

pub(super) async fn commit(State(services): State<Services>, user: User, body: Body) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::commit_upload(&body).ok_or_else(|| malformed(user.request_id))?;
    let committed = services
        .commits
        .commit_upload(actor(&user), request)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::committed(&committed)).into_response())
}

pub(super) async fn commit_batch(
    State(services): State<Services>,
    user: User,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let request = wire::batch_commit(&body).ok_or_else(|| malformed(user.request_id))?;
    let result = services
        .commits
        .commit_batch(actor(&user), request)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::batch_result(&result)).into_response())
}
