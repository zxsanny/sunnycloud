//! The invite handlers: bind, one `InviteService` call for the signed-in admin, map (01 section 3). The plaintext
//! token is in the create answer only.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{RawPathParams, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};

use super::wire;
use crate::json_body::{self, malformed};
use crate::registry::request::{Answer, BODY_LIMIT, Services, User, refused, uuid_param};

/// The web app's invite page: `<public_url>/invite#`, to which the token is appended as the fragment, so the token
/// never reaches a server log or a `Referer`. Built from the configured origin, never from a request's `Host`.
#[derive(Clone)]
pub(super) struct InvitePage(Arc<str>);

impl InvitePage {
    /// `public_url` has at most one trailing slash: the CSRF guard refuses any other form at startup.
    pub(super) fn new(public_url: &str) -> InvitePage {
        let origin = public_url.strip_suffix('/').unwrap_or(public_url);
        InvitePage(Arc::from(format!("{origin}/invite#")))
    }

    fn url(&self, token: &str) -> String {
        format!("{}{token}", self.0)
    }
}

pub(super) async fn create(
    State(services): State<Services>,
    Extension(page): Extension<InvitePage>,
    user: User,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let note = wire::invite_request(&body).ok_or_else(|| malformed(user.request_id))?;
    let invite = services
        .invites
        .create_invite(user.account, &note)
        .await
        .map_err(refused(user.request_id))?;
    Ok(Json(wire::created_invite(
        &invite,
        &page.url(invite.token.0.expose()),
    ))
    .into_response())
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
