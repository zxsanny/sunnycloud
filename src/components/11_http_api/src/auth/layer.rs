use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use contracts::{
    Authenticator, AuthenticatorError, Principal, PrincipalKind, Role, Scope, WorkerAuthenticator,
};
use sc_clock::Clock;
use sc_error::CorrelationId;

use super::format::{CredentialFormat, CredentialSource, bearer};
use crate::problem::ApiProblem;
use crate::registry::AuthClass;
use crate::services::ApiServices;

/// The caller of a user route, put into the request extensions after auth passes. Handlers read it with
/// `Extension<Authenticated>` and take the account from it, never from the path, query or body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authenticated {
    pub principal: Principal,
    /// True for an admin session: the one caller who may open a retained revision.
    pub retained_ok: bool,
    pub source: CredentialSource,
}

/// What an auth class asks of the request.
#[derive(Clone, Copy)]
enum Requirement {
    /// No check: `Public`, and `ShareGrant` which 06 resolves in the handler.
    Open,
    /// The worker secret, checked by 08 alone.
    Worker,
    /// A principal that 01's `require` accepts for the scope.
    User(Scope),
    /// A device token; 01 has no scope for it.
    Device,
}

fn requirement(class: AuthClass) -> Requirement {
    match class {
        AuthClass::Public | AuthClass::ShareGrant => Requirement::Open,
        AuthClass::Worker => Requirement::Worker,
        AuthClass::Session | AuthClass::SessionOnly => Requirement::User(Scope::SessionOnly),
        AuthClass::SessionOrDevice => Requirement::User(Scope::SessionOrDevice),
        AuthClass::Admin => Requirement::User(Scope::Admin),
        AuthClass::DeviceOnly => Requirement::Device,
    }
}

struct AuthState {
    requirement: Requirement,
    format: CredentialFormat,
    authenticator: Arc<dyn Authenticator>,
    worker_auth: Arc<dyn WorkerAuthenticator>,
    clock: Arc<dyn Clock>,
}

/// Puts the auth of `class` in front of `handler`. It runs only for a request that matched the route, so a
/// wrong method is still 405, and before any handler extractor, so no body byte is read for a refused request.
pub(crate) fn protect(
    handler: MethodRouter,
    class: AuthClass,
    services: &ApiServices,
    format: &CredentialFormat,
) -> MethodRouter {
    let requirement = requirement(class);
    if matches!(requirement, Requirement::Open) {
        return handler;
    }
    let state = Arc::new(AuthState {
        requirement,
        format: format.clone(),
        authenticator: Arc::clone(&services.authenticator),
        worker_auth: Arc::clone(&services.worker_auth),
        clock: Arc::clone(&services.clock),
    });
    handler.route_layer(from_fn_with_state(state, authorize))
}

async fn authorize(
    State(state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = request
        .extensions()
        .get::<CorrelationId>()
        .copied()
        .unwrap_or_else(|| CorrelationId::new(sc_ids::new_id(state.clock.as_ref())));
    match state.requirement {
        Requirement::Open => {}
        Requirement::Worker => {
            // Only the bearer goes to 08; a cookie or a missing header reaches it as an empty bearer so its
            // refusal count stays complete, and 01 is never asked.
            let presented = bearer(request.headers()).unwrap_or_default();
            match state.worker_auth.authenticate(presented).await {
                Ok(identity) => {
                    request.extensions_mut().insert(identity);
                }
                Err(refusal) => return ApiProblem::from_port(refusal, request_id).into_response(),
            }
        }
        Requirement::User(_) | Requirement::Device => {
            match user(&state, request.headers(), request_id).await {
                Ok(caller) => {
                    request.extensions_mut().insert(caller);
                }
                Err(problem) => return problem.into_response(),
            }
        }
    }
    next.run(request).await
}

async fn user(
    state: &AuthState,
    headers: &HeaderMap,
    request_id: CorrelationId,
) -> Result<Authenticated, ApiProblem> {
    let refuse = |error: AuthenticatorError| ApiProblem::from_port(error, request_id);
    let (credential, source) = state
        .format
        .credential(headers)
        .ok_or_else(|| refuse(AuthenticatorError::Unauthenticated))?;
    let principal = state
        .authenticator
        .authenticate(credential)
        .await
        .map_err(refuse)?;
    match state.requirement {
        Requirement::User(scope) => state
            .authenticator
            .require(&principal, scope)
            .map_err(refuse)?,
        Requirement::Device if principal.kind != PrincipalKind::Device => {
            return Err(refuse(AuthenticatorError::Forbidden));
        }
        Requirement::Device | Requirement::Open | Requirement::Worker => {}
    }
    let retained_ok = principal.kind == PrincipalKind::Session && principal.role == Role::Admin;
    Ok(Authenticated {
        principal,
        retained_ok,
        source,
    })
}
