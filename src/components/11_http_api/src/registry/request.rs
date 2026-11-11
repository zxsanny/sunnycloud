//! What the handlers of user and visitor routes read from a request beside its body, and the shapes they share.

use std::sync::Arc;

use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::Uri;
use axum::http::request::Parts;
use axum::response::Response;
use sc_clock::Clock;
use sc_error::{CorrelationId, DomainError};
use uuid::Uuid;

use super::RouteContext;
use crate::auth::Authenticated;
use crate::port_error::PortError;
use crate::problem::ApiProblem;
use crate::services::ApiServices;

pub(super) type Answer = Result<Response, ApiProblem>;

/// The body limit layer has already capped a request at its class, so reading a JSON body whole is bounded.
pub(super) const BODY_LIMIT: usize = usize::MAX;

/// What every user and visitor handler holds: the ports of 12.
pub(super) type Services = Arc<ApiServices>;

pub(super) fn services(context: &RouteContext) -> Services {
    Arc::clone(&context.services)
}

/// The id the request layer assigned; a router mounted without that layer takes a fresh one.
pub(super) fn request_id(parts: &Parts, clock: &dyn Clock) -> CorrelationId {
    parts
        .extensions
        .get::<CorrelationId>()
        .copied()
        .unwrap_or_else(|| CorrelationId::new(sc_ids::new_id(clock)))
}

/// A request reached a handler without something the layers in front of it always add.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct MissingLayer(pub(super) &'static str);

pub(super) fn missing_layer(what: &'static str, request_id: CorrelationId) -> ApiProblem {
    ApiProblem::new(
        DomainError::unexpected(MissingLayer(what), request_id),
        request_id,
    )
}

/// The caller of a user route: the account of the principal auth accepted, never one from the path, the query
/// or the body.
pub(super) struct User {
    pub(super) account: Uuid,
    pub(super) request_id: CorrelationId,
}

impl FromRequestParts<Services> for User {
    type Rejection = ApiProblem;

    async fn from_request_parts(parts: &mut Parts, state: &Services) -> Result<Self, ApiProblem> {
        let request_id = request_id(parts, state.clock.as_ref());
        match parts.extensions.get::<Authenticated>() {
            Some(caller) => Ok(User {
                account: caller.principal.account_id,
                request_id,
            }),
            None => Err(missing_layer("authenticated caller", request_id)),
        }
    }
}

pub(super) fn refused<E: PortError>(request_id: CorrelationId) -> impl FnOnce(E) -> ApiProblem {
    move |error| ApiProblem::from_port(error, request_id)
}

/// The `{name}` of the route as a UUID; anything else is a 400 for that field.
pub(super) fn uuid_param(
    params: &RawPathParams,
    name: &'static str,
    request_id: CorrelationId,
) -> Result<Uuid, ApiProblem> {
    params
        .iter()
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| Uuid::parse_str(value).ok())
        .ok_or_else(|| ApiProblem::validation(name, "not_a_uuid", request_id))
}

/// The first value of the query parameter `name`, as written (no percent decoding: every value a route reads
/// here is a UUID or a fixed word).
pub(super) fn query_value<'a>(uri: &'a Uri, name: &str) -> Option<&'a str> {
    uri.query()?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}
