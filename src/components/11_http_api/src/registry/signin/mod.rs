//! The Auth and Identity operations of `api_v1_surface.md` (01 section 3): the provider list, sign-in begin and
//! callback, the mobile code exchange, the refresh and the identity-link attempt.
//!
//! `complete_link` has no route: 01's route table lists none (open point 34 of the surface contract).

mod cookie;
mod handlers;
mod wire;

use axum::Extension;
use axum::routing::on;
use sc_error::ErrorCode;

use super::classes::{AuthClass, BodyClass, HttpMethod};
use super::request::services;
use super::spec::{Param, ParamType, RouteDef, RouteSpec};
use cookie::CookieNames;

const PROVIDERS: &str = "/api/v1/auth/providers";
const LOGIN: &str = "/api/v1/auth/login-attempts";
const CALLBACK: &str = "/api/v1/auth/providers/{provider}/callback";
const EXCHANGE: &str = "/api/v1/auth/mobile-code-exchanges";
const REFRESH: &str = "/api/v1/auth/refresh-tokens";
const LINK: &str = "/api/v1/identity-link-attempts";

const PROVIDER: &[Param] = &[Param::path("provider", ParamType::String)];

const LOGIN_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::RateLimited];
const CALLBACK_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::RateLimited];
const EXCHANGE_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::RateLimited];
const REFRESH_ERRORS: &[ErrorCode] = &[ErrorCode::Unauthenticated];
const LINK_ERRORS: &[ErrorCode] = &[ErrorCode::InvalidRequest, ErrorCode::RateLimited];

const LIST_PROVIDERS: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    PROVIDERS,
    "SignInService::list_providers",
    AuthClass::Public,
    BodyClass::None,
)
.summary("The enabled sign-in providers and whether first-run setup is still required")
.response(wire::PROVIDER_LIST);

const BEGIN: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    LOGIN,
    "SignInService::begin",
    AuthClass::Public,
    BodyClass::Auth,
)
.summary("Start a sign-in; sets the browser-binding cookie and returns the provider's URL")
.errors(LOGIN_ERRORS)
.request(wire::LOGIN_REQUEST)
.response(wire::AUTHORIZATION)
.success(201);

/// A GET that changes state by protocol: it consumes the state row and creates the session.
const CALLBACK_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    CALLBACK,
    "SignInService::complete",
    AuthClass::Public,
    BodyClass::None,
)
.summary("Where the provider sends the browser; 302 to the return path or to the SPA error route")
.params(PROVIDER)
.errors(CALLBACK_ERRORS)
.mutating()
.success(302);

const EXCHANGE_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    EXCHANGE,
    "SignInService::exchange_mobile_code",
    AuthClass::Public,
    BodyClass::Auth,
)
.summary("Trade the one-time mobile code and its PKCE verifier for session tokens")
.errors(EXCHANGE_ERRORS)
.request(wire::CODE_EXCHANGE_REQUEST)
.response(wire::SESSION_TOKENS);

const REFRESH_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    REFRESH,
    "SessionService::refresh",
    AuthClass::Public,
    BodyClass::Auth,
)
.summary("Rotate a refresh token for a new pair of session tokens")
.errors(REFRESH_ERRORS)
.request(wire::REFRESH_REQUEST)
.response(wire::SESSION_TOKENS);

const LINK_SPEC: RouteSpec = RouteSpec::new(
    HttpMethod::Post,
    LINK,
    "SignInService::begin_link",
    AuthClass::SessionOnly,
    BodyClass::Small,
)
.summary("Start linking another provider identity to the signed-in account")
.errors(LINK_ERRORS)
.request(wire::LINK_REQUEST)
.response(wire::AUTHORIZATION);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(LIST_PROVIDERS, |context, filter| {
            on(filter, handlers::providers).with_state(services(context))
        }),
        RouteDef::new(BEGIN, |context, filter| {
            on(filter, handlers::begin)
                .layer(Extension(CookieNames::new(&context.credentials)))
                .with_state(services(context))
        }),
        RouteDef::new(CALLBACK_SPEC, |context, filter| {
            on(filter, handlers::callback)
                .layer(Extension(CookieNames::new(&context.credentials)))
                .with_state(services(context))
        }),
        RouteDef::new(EXCHANGE_SPEC, |context, filter| {
            on(filter, handlers::exchange).with_state(services(context))
        }),
        RouteDef::new(REFRESH_SPEC, |context, filter| {
            on(filter, handlers::refresh).with_state(services(context))
        }),
        RouteDef::new(LINK_SPEC, |context, filter| {
            on(filter, handlers::begin_link)
                .layer(Extension(CookieNames::new(&context.credentials)))
                .with_state(services(context))
        }),
    ]
}
