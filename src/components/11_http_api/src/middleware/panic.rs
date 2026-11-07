use std::future::poll_fn;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, OnceLock};
use std::task::Poll;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use sc_error::CorrelationId;
use sc_observability::{NoLabels, log_warn};

use super::{Layers, internal_failure};

/// Lets layer 1, which sits outside the id layer, learn the id layer 3 assigned to the request.
#[derive(Debug, Clone, Default)]
pub(super) struct RequestIdSlot(Arc<OnceLock<CorrelationId>>);

impl RequestIdSlot {
    pub(super) fn assign(&self, id: CorrelationId) {
        if self.0.set(id).is_err() {
            log_warn!("request id was assigned twice", request_id = id);
        }
    }

    fn get(&self) -> Option<CorrelationId> {
        self.0.get().copied()
    }
}

/// Layer 1. A panic in anything below becomes a 500 problem body with the request's id and security headers,
/// and `http_panics_total` rises by one; the connection and its other streams are untouched. The payload is
/// dropped unread: it may hold request data, and the default panic hook has already printed it for the operator.
///
/// The answer is built here, not by the layers the panic unwound through, so it applies the security headers
/// itself and is not counted in the request families.
pub(super) async fn catch_panic(
    State(layers): State<Arc<Layers>>,
    mut request: Request,
    next: Next,
) -> Response {
    let slot = RequestIdSlot::default();
    request.extensions_mut().insert(slot.clone());
    let mut inner = Box::pin(next.run(request));
    let outcome = poll_fn(
        |cx| match catch_unwind(AssertUnwindSafe(|| inner.as_mut().poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(response)) => Poll::Ready(Ok(response)),
            Err(payload) => Poll::Ready(Err(payload)),
        },
    )
    .await;
    match outcome {
        Ok(response) => response,
        Err(payload) => {
            drop(payload);
            layers.metrics.panics.inc(NoLabels);
            let request_id = slot.get().unwrap_or_else(|| layers.fresh_id());
            internal_failure(request_id, "handler panicked")
        }
    }
}
