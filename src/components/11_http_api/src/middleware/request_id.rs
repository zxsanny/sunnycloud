use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use sc_observability::request_span;
use tracing::Instrument;

use super::Layers;
use super::panic::RequestIdSlot;
use crate::problem::insert_request_id;

/// Layer 3. A new UUIDv7 for every request: an incoming `X-Request-Id` is not read, so a client cannot choose
/// the id that appears in logs and error bodies (11 section 5; `api_v1_surface.md` open point 10).
///
/// The id is an extension (`Extension<CorrelationId>`) for the handlers and error bodies, the `request_id` of
/// every log event below this layer, and the `X-Request-Id` of the response.
pub(super) async fn request_id(
    State(layers): State<Arc<Layers>>,
    mut request: Request,
    next: Next,
) -> Response {
    let id = layers.fresh_id();
    if let Some(slot) = request.extensions().get::<RequestIdSlot>() {
        slot.assign(id);
    }
    request.extensions_mut().insert(id);
    let mut response = next.run(request).instrument(request_span(id)).await;
    insert_request_id(response.headers_mut(), id);
    response
}
