//! What the handlers of user and visitor routes read from a request beside its body, and the shapes they share.

use std::sync::Arc;

use std::str::FromStr;

use axum::body::Body;
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Uri, header};
use axum::response::Response;
use contracts::{ByteStream, Principal, StreamError};
use http_body_util::BodyExt;
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
/// or the body. `retained_ok` is true for an admin session only.
pub(super) struct User {
    pub(super) account: Uuid,
    pub(super) principal: Principal,
    pub(super) retained_ok: bool,
    pub(super) request_id: CorrelationId,
}

impl FromRequestParts<Services> for User {
    type Rejection = ApiProblem;

    async fn from_request_parts(parts: &mut Parts, state: &Services) -> Result<Self, ApiProblem> {
        let request_id = request_id(parts, state.clock.as_ref());
        match parts.extensions.get::<Authenticated>() {
            Some(caller) => Ok(User {
                account: caller.principal.account_id,
                principal: caller.principal.clone(),
                retained_ok: caller.retained_ok,
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

/// `name` as a number; absent is `None`, a value that is not one is a 400 for that field.
pub(super) fn query_number<T: FromStr>(
    uri: &Uri,
    name: &'static str,
    request_id: CorrelationId,
) -> Result<Option<T>, ApiProblem> {
    query_value(uri, name)
        .map(|text| {
            text.parse()
                .map_err(|_| ApiProblem::validation(name, "malformed", request_id))
        })
        .transpose()
}

/// `name` as a UUID; absent is `None`, anything else is a 400 for that field.
pub(super) fn query_uuid(
    uri: &Uri,
    name: &'static str,
    request_id: CorrelationId,
) -> Result<Option<Uuid>, ApiProblem> {
    query_value(uri, name)
        .map(|text| {
            Uuid::parse_str(text)
                .map_err(|_| ApiProblem::validation(name, "not_a_uuid", request_id))
        })
        .transpose()
}

/// `name` as the text a client percent-encoded (an opaque page key, a device label); absent is `None`, a bad
/// escape or bytes that are not UTF-8 are a 400 for that field. A `+` stays a `+`.
pub(super) fn query_text(
    uri: &Uri,
    name: &'static str,
    request_id: CorrelationId,
) -> Result<Option<String>, ApiProblem> {
    query_value(uri, name)
        .map(|text| {
            percent_decode(text)
                .ok_or_else(|| ApiProblem::validation(name, "malformed", request_id))
        })
        .transpose()
}

fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.bytes();
    while let Some(byte) = rest.next() {
        if byte == b'%' {
            let high = hex_digit(rest.next()?)?;
            let low = hex_digit(rest.next()?)?;
            bytes.push(high << 4 | low);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

/// The 32 bytes of 64 hex digits, upper or lower case (a block or content hash on the wire).
pub(super) fn digest(text: &str) -> Option<[u8; 32]> {
    let digits = text.as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks_exact(2)) {
        *byte = hex_digit(pair[0])? << 4 | hex_digit(pair[1])?;
    }
    Some(bytes)
}

fn hex_digit(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// The declared `Content-Length`; `None` when absent or not a number.
pub(super) fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// A request body as the neutral stream the ports take: chunks as they arrive, never collected.
pub(super) fn byte_stream(body: Body) -> ByteStream {
    Box::pin(
        body.map_err(|failure| StreamError::Source(Box::new(failure)))
            .into_data_stream(),
    )
}
