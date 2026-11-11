//! Feature `test-clock` only: the clock endpoints of the `-test` image (FT-N-35, `api_v1_surface.md`, Test clock
//! rows). `GET /internal/v1/test/clock-offset` is a registry operation on the internal listener behind the worker
//! bearer; `POST /test/clock` is a router of its own on the `:9091` listener, outside the document.
//!
//! Release builds do not compile this module, so they contain neither route nor the listener.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::routing::{MethodFilter, MethodRouter, on, post};
use sc_clock::{ClockError, OffsetClock};
use sc_error::CorrelationId;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::json_body::{self, fields, malformed};
use crate::problem::ApiProblem;
use crate::registry::{
    AuthClass, BodyClass, HttpMethod, RouteContext, RouteDef, RouteSpec, SchemaRef,
};

/// `{"advance_seconds": N}` is a few dozen bytes; anything near this is not a clock request.
const MAX_BODY: usize = 1024;

const CLOCK_OFFSET: SchemaRef = SchemaRef {
    name: "ClockOffset",
    build: || {
        json!({
            "type": "object",
            "required": ["offset_seconds"],
            "properties": {"offset_seconds": {"type": "integer", "minimum": 0}}
        })
    },
};

const OFFSET_READ: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    "/internal/v1/test/clock-offset",
    "OffsetClock::offset",
    AuthClass::Worker,
    BodyClass::None,
)
.internal()
.summary("The shared test-clock offset, for the worker supervisor")
.response(CLOCK_OFFSET);

/// The clock-offset read, registered only in test builds.
pub(crate) fn routes() -> Vec<RouteDef> {
    vec![RouteDef::new(OFFSET_READ, mount_offset)]
}

fn mount_offset(context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    let clock = Arc::clone(&context.services.offset_clock);
    on(filter, move || {
        let clock = Arc::clone(&clock);
        async move { offset_body(&clock) }
    })
}

fn offset_body(clock: &OffsetClock) -> Json<Value> {
    Json(json!({"offset_seconds": clock.offset().as_secs()}))
}

/// The `:9091` listener of a test build.
pub(crate) struct TestClockListener {
    pub(crate) listener: TcpListener,
    pub(crate) addr: SocketAddr,
    pub(crate) app: Router,
}

/// `POST /test/clock`. It has no credential: the documents define none for it (`api_v1_surface.md`, open point
/// 7), and the listener exists only in test images, on a port no proxy routes.
pub(crate) fn router(clock: Arc<OffsetClock>) -> Router {
    Router::new()
        .route("/test/clock", post(advance))
        .with_state(clock)
}

async fn advance(
    State(clock): State<Arc<OffsetClock>>,
    request_id: Option<Extension<CorrelationId>>,
    body: Body,
) -> Result<Json<Value>, ApiProblem> {
    let request_id = request_id.map_or_else(
        || CorrelationId::new(sc_ids::new_id(clock.as_ref())),
        |Extension(id)| id,
    );
    let body = json_body::read(body, MAX_BODY, request_id).await?;
    let seconds = fields(&body, ["advance_seconds"])
        .and_then(|[seconds]| seconds.as_i64())
        .ok_or_else(|| malformed(request_id))?;
    clock
        .advance_seconds(seconds)
        .map_err(|error| match error {
            ClockError::NegativeAdvance => {
                ApiProblem::validation("advance_seconds", "negative", request_id)
            }
            ClockError::OffsetOverflow => {
                ApiProblem::validation("advance_seconds", "out_of_range", request_id)
            }
        })?;
    Ok(offset_body(&clock))
}
