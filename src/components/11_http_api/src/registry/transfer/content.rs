//! The content routes of 03 section 4: the current revision of a node, and one revision. Both answer through the
//! shared header mapper. Only an admin session may open a revision that is no longer current (`retained_ok`).

use axum::extract::{RawPathParams, State};
use axum::http::{HeaderMap, Method};
use contracts::RevisionOrCurrent;

use crate::content::{content_response, download_options};
use crate::registry::request::{Answer, Services, User, uuid_param};

pub(super) async fn node_content(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    method: Method,
    headers: HeaderMap,
) -> Answer {
    let node = uuid_param(&params, "node_id", user.request_id)?;
    let opened = services
        .downloads
        .open_revision_content(
            user.account,
            RevisionOrCurrent::Current(node),
            download_options(&headers),
        )
        .await;
    Ok(content_response(opened, &method, user.request_id))
}

pub(super) async fn revision_content(
    State(services): State<Services>,
    user: User,
    params: RawPathParams,
    method: Method,
    headers: HeaderMap,
) -> Answer {
    let revision = uuid_param(&params, "revision_id", user.request_id)?;
    let mut options = download_options(&headers);
    options.retained_ok = user.retained_ok;
    let opened = services
        .downloads
        .open_revision_content(user.account, RevisionOrCurrent::Revision(revision), options)
        .await;
    Ok(content_response(opened, &method, user.request_id))
}
