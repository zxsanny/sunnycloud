//! The visitor routes of a public link: open, password verification and content. Nothing here has an account;
//! 06 decides from the token, the grant and the client address, and 11 streams what it allows through 03.
//!
//! Every missing, expired, revoked or deleted link is the one 404 of `ApiProblem::not_found`, so no answer
//! tells them apart (AC-12). The token is read from the path into a `RawToken` and goes nowhere but 06: no log
//! line or metric label is built from the path (the route template is the label).

use axum::Json;
use axum::body::Body;
use axum::extract::{FromRequestParts, RawPathParams, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use contracts::{AccessGrant, RevisionOrCurrent, ShareOpen};
use sc_error::{CorrelationId, DomainError, ErrorCode};
use sc_ids::{RawToken, TokenKind};
use std::net::IpAddr;

use super::{grant, wire};
use crate::content::{content_response, download_options, starts_download};
use crate::json_body::{self, malformed};
use crate::middleware::ClientIp;
use crate::problem::ApiProblem;
use crate::registry::media::wire::{media_type, thumbnail_kind};
use crate::registry::request::{
    Answer, BODY_LIMIT, Services, missing_layer, query_value, refused, request_id,
};

const TOKEN_PARAM: &str = "token";
const SIZE_PARAM: &str = "size";

/// The caller of a visitor route: the link token of the path, the client address the abuse limits count, and the
/// grant cookie when the request carries one.
pub(super) struct Visitor {
    token: RawToken,
    client_ip: IpAddr,
    grant: Option<AccessGrant>,
    request_id: CorrelationId,
}

impl FromRequestParts<Services> for Visitor {
    type Rejection = ApiProblem;

    async fn from_request_parts(parts: &mut Parts, state: &Services) -> Result<Self, ApiProblem> {
        let request_id = request_id(parts, state.clock.as_ref());
        let token = RawPathParams::from_request_parts(parts, state)
            .await
            .ok()
            .and_then(|params| {
                params
                    .iter()
                    .find(|(name, _)| *name == TOKEN_PARAM)
                    .map(|(_, value)| value.to_owned())
            })
            // A path that is not text names no link: it is the uniform 404, like any other unknown token.
            .ok_or_else(|| ApiProblem::not_found(request_id))?;
        let client_ip = parts
            .extensions
            .get::<ClientIp>()
            .map(|client| client.0)
            .ok_or_else(|| missing_layer("client address", request_id))?;
        Ok(Visitor {
            token: RawToken::from_presented(TokenKind::Share, token),
            client_ip,
            grant: grant::from_headers(&parts.headers),
            request_id,
        })
    }
}

pub(super) async fn open(State(services): State<Services>, visitor: Visitor) -> Answer {
    let opened = services
        .public_shares
        .open(&visitor.token, visitor.client_ip)
        .await
        .map_err(refused(visitor.request_id))?;
    match opened {
        ShareOpen::Available {
            name,
            size,
            media_type,
            expires_at,
        } => Ok(Json(wire::share_open(
            &name,
            size,
            media_type.as_deref(),
            expires_at,
        ))
        .into_response()),
        ShareOpen::PasswordRequired => Err(ApiProblem::code(
            ErrorCode::PasswordRequired,
            visitor.request_id,
        )),
        ShareOpen::NotFound => Err(ApiProblem::not_found(visitor.request_id)),
    }
}

/// A correct password answers 204 and sets the grant cookie; every refusal is 06's (wrong password and
/// password required are 401 `password_required`, an unknown link the uniform 404, too many tries 429).
pub(super) async fn verify_password(
    State(services): State<Services>,
    visitor: Visitor,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, visitor.request_id).await?;
    let password = wire::password(&body).ok_or_else(|| malformed(visitor.request_id))?;
    let granted = services
        .public_shares
        .verify_password(&visitor.token, password, visitor.client_ip)
        .await
        .map_err(refused(visitor.request_id))?;
    let cookie = grant::set_cookie(&granted, visitor.token.expose(), services.clock.now_utc())
        .map_err(|broken| {
            ApiProblem::new(
                DomainError::unexpected(broken, visitor.request_id),
                visitor.request_id,
            )
        })?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(response)
}

/// 06 decides whether the link and its grant allow the file; 03 serves its current revision. Only the bytes of
/// a link may be read by a page of another origin, and only without credentials.
pub(super) async fn content(
    State(services): State<Services>,
    visitor: Visitor,
    method: Method,
    headers: HeaderMap,
) -> Answer {
    // A HEAD sends no bytes, so it is no download start and writes no `share_accessed`.
    let starts = method != Method::HEAD && starts_download(&headers);
    let allowed = services
        .public_shares
        .authorize_content(
            &visitor.token,
            visitor.grant.as_ref(),
            visitor.client_ip,
            starts,
        )
        .await
        .map_err(refused(visitor.request_id))?;
    let opened = services
        .downloads
        .open_revision_content(
            allowed.account_id,
            RevisionOrCurrent::Revision(allowed.revision_id),
            download_options(&headers),
        )
        .await;
    let mut response = content_response(opened, &method, visitor.request_id);
    allow_any_origin(&mut response);
    Ok(response)
}

/// 06 authorises the visitor and names the file with its account, 08 finds the derived blob of that file, and 03
/// streams it. Unlike the content route it carries no cross-origin header: only the bytes of a link are for pages
/// of other origins (`api_v1_surface.md`).
pub(super) async fn thumbnail(
    State(services): State<Services>,
    visitor: Visitor,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Answer {
    let kind = thumbnail_kind(query_value(&uri, SIZE_PARAM))
        .ok_or_else(|| ApiProblem::validation(SIZE_PARAM, "invalid", visitor.request_id))?;
    let shared = services
        .public_shares
        .authorize_derived(&visitor.token, visitor.grant.as_ref(), visitor.client_ip)
        .await
        .map_err(refused(visitor.request_id))?;
    let blob = services
        .derived
        .open_derived_for(&shared.file, kind)
        .await
        .map_err(refused(visitor.request_id))?;
    let opened = services
        .downloads
        .open_blob_content(
            shared.account_id,
            blob,
            Some(media_type(kind).to_owned()),
            download_options(&headers),
        )
        .await;
    Ok(content_response(opened, &method, visitor.request_id))
}

/// `Access-Control-Allow-Origin: *` on the bytes served (200, 206, 304), never with credentials.
fn allow_any_origin(response: &mut Response) {
    let status = response.status();
    if status.is_success() || status == StatusCode::NOT_MODIFIED {
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    }
}
