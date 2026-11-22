//! One handler per sign-in operation: bind, call the one `SignInService` or `SessionService` method, map the
//! result (01 section 3). The browser carries a binding cookie from `begin` to the callback; the session cookie,
//! the redirects and the one-time mobile code are delivered here and decided by 01.
//!
//! Nothing here is logged: a token, a code, a return path or a callback parameter reaches only the port and the
//! response.

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Extension, RawPathParams, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use contracts::{
    AuthenticatorError, AuthorizationRedirect, BrowserBinding, CallbackParams, Credential,
    Principal, Scope, SessionError, SignInError, SignInOutcome,
};
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};
use sc_ids::{RawToken, TokenKind};

use super::cookie::{CookieNames, NotHeaderSafe, REFRESH_COOKIE, location, with_code};
use super::wire;
use crate::csrf::CsrfGuard;
use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::request::{
    Anonymous, Answer, BODY_LIMIT, Services, User, cookie_value, host, query_pairs, refused,
};

/// The SPA route a failed sign-in lands on; the reason is its `error` parameter.
const ERROR_ROUTE: &str = "/login";
/// The SPA route a link attempt lands on, whatever its outcome: `linked=1` or `error=<reason>`.
const ACCOUNT_ROUTE: &str = "/account";
const NOT_INVITED: &str = "invitation_required";
const IDENTITY_TAKEN: &str = "identity_taken";

pub(super) async fn providers(State(services): State<Services>, caller: Anonymous) -> Answer {
    let list = services
        .sign_in
        .list_providers()
        .await
        .map_err(refused(caller.request_id))?;
    Ok(Json(wire::providers(&list)).into_response())
}

/// 201 with the provider's authorization URL; the binding cookie ties the callback to this browser.
pub(super) async fn begin(
    State(services): State<Services>,
    Extension(cookies): Extension<CookieNames>,
    caller: Anonymous,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Answer {
    let request_id = caller.request_id;
    let body = json_body::read(body, BODY_LIMIT, request_id).await?;
    let host = host(&headers, &uri)
        .ok_or_else(|| ApiProblem::validation("host", "missing", request_id))?;
    let request =
        wire::login_request(&body, host, caller.client_ip).ok_or_else(|| malformed(request_id))?;
    let redirect = services
        .sign_in
        .begin(request)
        .await
        .map_err(refused(request_id))?;
    authorization(&cookies, &redirect, StatusCode::CREATED, request_id)
}

/// The same answer for the signed-in account that links another provider (`begin_link`).
pub(super) async fn begin_link(
    State(services): State<Services>,
    Extension(cookies): Extension<CookieNames>,
    user: User,
    body: Body,
) -> Answer {
    let body = json_body::read(body, BODY_LIMIT, user.request_id).await?;
    let provider = wire::link_request(&body).ok_or_else(|| malformed(user.request_id))?;
    let redirect = services
        .sign_in
        .begin_link(&user.principal, &provider)
        .await
        .map_err(refused(user.request_id))?;
    authorization(&cookies, &redirect, StatusCode::OK, user.request_id)
}

fn authorization(
    cookies: &CookieNames,
    redirect: &AuthorizationRedirect,
    status: StatusCode,
    request_id: CorrelationId,
) -> Answer {
    let cookie = cookies
        .binding_cookie(&redirect.browser_binding)
        .map_err(|broken| broken_answer(broken, request_id))?;
    let mut response = (status, Json(wire::authorization(redirect))).into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(response)
}

/// The provider sends the browser here. A request that carries a live session cookie is the end of an identity
/// link (`complete_link`): a redirect to the account page, `linked=1` or `error=<reason>`. Any other request is a
/// sign-in: a session outcome sets the session and refresh cookies and redirects to the return path; a mobile
/// outcome redirects there with the one-time code; a refusal the browser should see is a redirect to the SPA
/// error route. Everything else, 429 included, is the port's own answer.
pub(super) async fn callback(
    State(services): State<Services>,
    Extension(cookies): Extension<CookieNames>,
    caller: Anonymous,
    params: RawPathParams,
    uri: Uri,
    headers: HeaderMap,
) -> Answer {
    let request_id = caller.request_id;
    let provider = params
        .iter()
        .find(|(name, _)| *name == "provider")
        .map(|(_, value)| value)
        .ok_or_else(|| ApiProblem::validation("provider", "missing", request_id))?;
    let pairs = query_pairs(&uri)
        .ok_or_else(|| ApiProblem::validation("query", "malformed", request_id))?;
    let host = host(&headers, &uri)
        .ok_or_else(|| ApiProblem::validation("host", "missing", request_id))?;
    // A browser without the cookie presents an empty binding, which matches no state row: 01 decides.
    let binding = BrowserBinding(
        cookie_value(&headers, cookies.binding_name())
            .unwrap_or_default()
            .to_owned(),
    );
    if let Some(principal) = link_caller(&services, &cookies, &headers, request_id).await? {
        let linked = services
            .sign_in
            .complete_link(
                &principal,
                provider,
                CallbackParams(pairs.clone()),
                &binding,
                &host,
                caller.client_ip,
            )
            .await;
        // A state that is not a link state of this account is looked up by another hash and consumes nothing, so
        // a browser that also holds a session (a web sign-in open next to a mobile one) still finishes its sign-in.
        if !matches!(linked, Err(SignInError::UntrustedState)) {
            return link_answer(linked.map(|_| ()), request_id);
        }
    }
    let outcome = services
        .sign_in
        .complete(
            provider,
            CallbackParams(pairs),
            &binding,
            &host,
            caller.client_ip,
        )
        .await;
    match outcome {
        Ok(SignInOutcome::Session { tokens, return_to }) => {
            let session = cookies
                .session_cookie(&tokens)
                .map_err(|broken| broken_answer(broken, request_id))?;
            let refresh = cookies
                .refresh_cookie(&tokens)
                .map_err(|broken| broken_answer(broken, request_id))?;
            let mut response = redirect(&return_to, request_id)?;
            let headers = response.headers_mut();
            headers.append(header::SET_COOKIE, session);
            headers.append(header::SET_COOKIE, refresh);
            Ok(response)
        }
        Ok(SignInOutcome::MobileCode { code, return_to }) => {
            redirect(&with_code(&return_to, code.expose()), request_id)
        }
        Err(other) => match redirect_reason(&other) {
            Some(reason) => error_redirect(ERROR_ROUTE, reason, request_id),
            None => Err(ApiProblem::from_port(other, request_id)),
        },
    }
}

/// The account that began a link: the callback is a public route, so it authenticates the session cookie itself,
/// through the same port and scope as a `SessionOnly` route. No cookie, or one 01 no longer accepts (a stale
/// cookie of a signed-out browser), is a plain sign-in; any other failure is the port's own answer.
async fn link_caller(
    services: &Services,
    cookies: &CookieNames,
    headers: &HeaderMap,
    request_id: CorrelationId,
) -> Result<Option<Principal>, ApiProblem> {
    let Some(value) =
        cookie_value(headers, cookies.session_name()).filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let credential = Credential::Session(RawToken::from_presented(
        TokenKind::Session,
        value.to_owned(),
    ));
    match services.authenticator.authenticate(credential).await {
        Ok(principal) => {
            services
                .authenticator
                .require(&principal, Scope::SessionOnly)
                .map_err(refused(request_id))?;
            Ok(Some(principal))
        }
        Err(AuthenticatorError::Unauthenticated) => Ok(None),
        Err(other) => Err(ApiProblem::from_port(other, request_id)),
    }
}

/// An identity link ends on the account page: `linked=1` (also for an identity the account already holds), or the
/// reason it was refused. The reasons of a sign-in refusal keep their words; an identity that belongs to another
/// account has no wire code, so it is named here.
fn link_answer(result: Result<(), SignInError>, request_id: CorrelationId) -> Answer {
    match result {
        Ok(()) => redirect(&format!("{ACCOUNT_ROUTE}?linked=1"), request_id),
        Err(SignInError::IdentityTaken) => {
            error_redirect(ACCOUNT_ROUTE, IDENTITY_TAKEN, request_id)
        }
        Err(other) => match redirect_reason(&other) {
            Some(reason) => error_redirect(ACCOUNT_ROUTE, reason, request_id),
            None => Err(ApiProblem::from_port(other, request_id)),
        },
    }
}

/// The reason a browser is told when the callback refuses it with a redirect; the other refusals keep the port's
/// answer (429, 503, 500).
fn redirect_reason(error: &SignInError) -> Option<&str> {
    match error {
        SignInError::UntrustedState => Some(ErrorCode::InvalidRequest.as_str()),
        SignInError::TrustedFailure { code } => Some(code.as_str()),
        SignInError::InvitationRequired => Some(NOT_INVITED),
        _ => None,
    }
}

fn redirect(path: &str, request_id: CorrelationId) -> Answer {
    let target = location(path).map_err(|broken| broken_answer(broken, request_id))?;
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(header::LOCATION, target);
    Ok(response)
}

fn error_redirect(route: &str, reason: &str, request_id: CorrelationId) -> Answer {
    redirect(&format!("{route}?error={reason}"), request_id)
}

/// A relative return path (or a token) 01 produced that a header cannot hold is a broken 01: opaque 500, logged.
fn broken_answer(broken: NotHeaderSafe, request_id: CorrelationId) -> ApiProblem {
    ApiProblem::new(DomainError::unexpected(broken, request_id), request_id)
}

/// The mobile app trades its one-time code and PKCE verifier for tokens, delivered in the body.
pub(super) async fn exchange(
    State(services): State<Services>,
    caller: Anonymous,
    body: Body,
) -> Answer {
    let request_id = caller.request_id;
    let body = json_body::read(body, BODY_LIMIT, request_id).await?;
    let (code, verifier) = wire::code_exchange(&body).ok_or_else(|| malformed(request_id))?;
    let tokens = services
        .sign_in
        .exchange_mobile_code(&code, &verifier, caller.client_ip)
        .await
        .map_err(|error| match error {
            // Single use, PKCE mismatch and expiry look alike to the caller; `sc_error` has no row for the case.
            SignInError::InvalidCode => ApiProblem::validation("code", "invalid", request_id),
            other => ApiProblem::from_port(other, request_id),
        })?;
    Ok(Json(wire::tokens(&tokens)).into_response())
}

/// A refresh token for a new pair, delivered in the body; reuse and unknown tokens are 01's 401.
pub(super) async fn refresh(
    State(services): State<Services>,
    caller: Anonymous,
    body: Body,
) -> Answer {
    let request_id = caller.request_id;
    let body = json_body::read(body, BODY_LIMIT, request_id).await?;
    let token = wire::refresh_request(&body).ok_or_else(|| malformed(request_id))?;
    let tokens = services
        .sessions
        .refresh(&token)
        .await
        .map_err(refused(request_id))?;
    Ok(Json(wire::tokens(&tokens)).into_response())
}

/// The browser trades its refresh cookie for a new session cookie and a new refresh cookie, and gets no body.
/// The cookie is sent only to this route and is `SameSite=Strict`; the origin is checked here as well, because
/// a public route runs no CSRF guard. A refresh token 01 refuses (reused, expired, unknown) is 401 and the cookie
/// is cleared; any other failure leaves it, so a database outage does not sign the browser out.
pub(super) async fn renew(
    State(services): State<Services>,
    Extension(cookies): Extension<CookieNames>,
    Extension(csrf): Extension<Arc<CsrfGuard>>,
    caller: Anonymous,
    headers: HeaderMap,
) -> Answer {
    let request_id = caller.request_id;
    if !csrf.proves_same_origin(&headers) {
        let rejected = BusinessError::new(ErrorCode::CsrfRejected, BusinessParams::default());
        return Err(ApiProblem::new(DomainError::Business(rejected), request_id));
    }
    let presented = cookie_value(&headers, REFRESH_COOKIE)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiProblem::from_port(SessionError::Unauthenticated, request_id))?;
    let token = RawToken::from_presented(TokenKind::Refresh, presented.to_owned());
    match services.sessions.refresh(&token).await {
        Ok(tokens) => {
            let session = cookies
                .session_cookie(&tokens)
                .map_err(|broken| broken_answer(broken, request_id))?;
            let refresh = cookies
                .refresh_cookie(&tokens)
                .map_err(|broken| broken_answer(broken, request_id))?;
            let mut response = StatusCode::NO_CONTENT.into_response();
            let headers = response.headers_mut();
            headers.append(header::SET_COOKIE, session);
            headers.append(header::SET_COOKIE, refresh);
            Ok(response)
        }
        Err(error) => {
            let mut response = ApiProblem::from_port(error, request_id).into_response();
            if response.status() == StatusCode::UNAUTHORIZED {
                response
                    .headers_mut()
                    .append(header::SET_COOKIE, CookieNames::cleared_refresh_cookie());
            }
            Ok(response)
        }
    }
}
