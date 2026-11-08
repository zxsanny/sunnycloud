use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{FromRequestParts, RawPathParams, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};
use sc_observability::NoLabels;

use super::BodyLimits;
use super::body::{AbortedBody, Cut, GuardedBody, Trip};
use crate::problem::ApiProblem;
use crate::registry::{BodyClass, Timing};

/// The status head of an aborted answer. The reset that follows it is what the peer sees end the request.
const ABORT_STATUS: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;
const KIND_PARAM: &str = "kind";
const WAIT_PARAM: &str = "wait";

/// What the layer needs to know about the route it guards.
pub(super) struct Route {
    pub(super) limits: Arc<BodyLimits>,
    pub(super) class: BodyClass,
    pub(super) timing: Timing,
}

pub(super) async fn enforce(
    State(route): State<Arc<Route>>,
    request: Request,
    next: Next,
) -> Response {
    let limits = &route.limits;
    let request_id = request
        .extensions()
        .get::<CorrelationId>()
        .copied()
        .unwrap_or_else(|| CorrelationId::new(sc_ids::new_id(limits.clock.as_ref())));
    let (mut parts, body) = request.into_parts();
    let kind = match output_kind(&route, &mut parts).await {
        Ok(kind) => kind,
        Err(reason) => {
            return ApiProblem::new(DomainError::unexpected(reason, request_id), request_id)
                .into_response();
        }
    };
    let wait = (route.timing == Timing::LongPoll)
        .then(|| wait_of(parts.uri.query()))
        .flatten();
    let plan = limits
        .settings
        .plan(route.class, route.timing, kind.as_deref(), wait);
    let declared = declared_length(&parts.headers);
    if route.class == BodyClass::Block && declared.is_none() {
        return length_required(request_id);
    }
    if declared.is_some_and(|declared| declared > plan.ceiling) {
        return too_large(limits, request_id);
    }

    let trip = Arc::new(Trip::default());
    let guarded = GuardedBody::new(body, plan.ceiling, plan.idle, Arc::clone(&trip));
    let request = Request::from_parts(parts, Body::new(guarded));
    let handled = next.run(request);
    tokio::pin!(handled);
    let cut = tokio::select! {
        response = &mut handled => match trip.cut() {
            None => return response,
            Some(cut) => Some(cut),
        },
        cut = trip.tripped() => Some(cut),
        () = expired(plan.request) => None,
    };
    match cut {
        Some(Cut::TooLarge) => too_large(limits, request_id),
        Some(Cut::Idle) | None => aborted(),
    }
}

/// The `{kind}` of a worker output route; nothing for any other class.
async fn output_kind(route: &Route, parts: &mut Parts) -> Result<Option<String>, &'static str> {
    if route.class != BodyClass::WorkerOutput {
        return Ok(None);
    }
    let params = RawPathParams::from_request_parts(parts, &())
        .await
        .map_err(|_| "worker output route has no path parameters")?;
    Ok(params
        .iter()
        .find(|(name, _)| *name == KIND_PARAM)
        .map(|(_, value)| value.to_owned()))
}

/// The `wait` of a long-poll, in whole seconds (the unit `longpoll_max_wait` is served in). A value that is
/// not a number is left to the handler to refuse.
fn wait_of(query: Option<&str>) -> Option<Duration> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == WAIT_PARAM)
        .and_then(|(_, value)| value.parse().ok())
        .map(Duration::from_secs)
}

fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

async fn expired(request: Option<Duration>) {
    match request {
        Some(limit) => tokio::time::sleep(limit).await,
        None => std::future::pending().await,
    }
}

fn too_large(limits: &BodyLimits, request_id: CorrelationId) -> Response {
    limits.metrics.body_limit_exceeded.inc(NoLabels);
    let error = BusinessError::new(ErrorCode::SizeTooLarge, BusinessParams::default());
    ApiProblem::new(DomainError::Business(error), request_id).into_response()
}

fn length_required(request_id: CorrelationId) -> Response {
    let error = BusinessError::new(ErrorCode::LengthRequired, BusinessParams::default());
    ApiProblem::new(DomainError::Business(error), request_id).into_response()
}

fn aborted() -> Response {
    let mut response = Response::new(Body::new(AbortedBody));
    *response.status_mut() = ABORT_STATUS;
    response
}
