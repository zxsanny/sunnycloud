//! Feature `test-clock` only: the one-shot worker transfer hold of the `-test` image (AZ-4381; decision D1 of the
//! AZ-3878 worker suite). `POST /test/worker-hold` on the `:9091` listener arms it. The next worker input or output
//! body that matches passes `after_bytes` bytes and then waits until `DELETE /test/worker-hold`, so a test can
//! kill the supervisor inside the fetch or inside the post. A hold nobody releases lets go after [`HOLD_LIMIT`] of
//! monotonic time; the offset clock plays no part, so moving the test clock never releases it.
//!
//! Release builds do not compile this module: no route, no state, no wrapper.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use contracts::{ByteStream, StreamError};
use futures_core::Stream;
use sc_clock::OffsetClock;
use sc_error::CorrelationId;
use serde_json::{Value, json};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;

/// How long an unreleased hold keeps its stream.
pub const HOLD_LIMIT: Duration = Duration::from_secs(60);

/// `{"route": "output", "kind": "thumb", "after_bytes": N}` is a few dozen bytes.
const MAX_BODY: usize = 1024;

/// Which worker transfer a hold waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldRoute {
    /// `GET /api/v1/worker-jobs/{job}/input`: the API's answer body.
    Input,
    /// `PUT /api/v1/worker-jobs/{job}/outputs/{kind}`: the request body the API reads.
    Output,
}

/// An armed hold: the route, for an output optionally one kind, and the bytes that pass before it waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldRequest {
    pub route: HoldRoute,
    pub kind: Option<String>,
    pub after_bytes: u64,
}

/// What `GET /test/worker-hold` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HoldStatus {
    pub armed: bool,
    pub held: bool,
    pub job_id: Option<Uuid>,
    pub bytes: u64,
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    Armed(HoldRequest),
    /// A stream took the hold; `held` once it has passed its bytes and waits.
    Taken {
        job_id: Uuid,
        bytes: u64,
        held: bool,
    },
}

/// The one hold of a test-image API.
#[derive(Debug)]
pub struct WorkerHold {
    state: Mutex<Phase>,
    released: Notify,
    limit: Duration,
}

impl Default for WorkerHold {
    fn default() -> WorkerHold {
        WorkerHold::with_limit(HOLD_LIMIT)
    }
}

impl WorkerHold {
    /// A hold whose unreleased streams let go after `limit`.
    pub fn with_limit(limit: Duration) -> WorkerHold {
        WorkerHold {
            state: Mutex::new(Phase::Idle),
            released: Notify::new(),
            limit,
        }
    }

    fn state(&self) -> MutexGuard<'_, Phase> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Arms the hold; `false` when one is already armed or taken.
    pub fn arm(&self, request: HoldRequest) -> bool {
        let mut state = self.state();
        if !matches!(*state, Phase::Idle) {
            return false;
        }
        *state = Phase::Armed(request);
        true
    }

    pub fn status(&self) -> HoldStatus {
        match &*self.state() {
            Phase::Idle => HoldStatus::default(),
            Phase::Armed(_) => HoldStatus {
                armed: true,
                ..HoldStatus::default()
            },
            Phase::Taken {
                job_id,
                bytes,
                held,
            } => HoldStatus {
                armed: false,
                held: *held,
                job_id: Some(*job_id),
                bytes: *bytes,
            },
        }
    }

    /// Lets a waiting stream go on and disarms.
    pub fn release(&self) {
        *self.state() = Phase::Idle;
        self.released.notify_waiters();
    }

    /// `body` unchanged, unless the armed hold matches `route` and `kind`: then the hold is taken (one shot) and the
    /// stream passes the hold's bytes and waits for the release.
    pub fn wrap(
        self: &Arc<Self>,
        route: HoldRoute,
        kind: Option<&str>,
        job_id: Uuid,
        body: ByteStream,
    ) -> ByteStream {
        let mut state = self.state();
        let Phase::Armed(request) = &*state else {
            return body;
        };
        let kind_matches = request
            .kind
            .as_deref()
            .is_none_or(|want| Some(want) == kind);
        if request.route != route || !kind_matches {
            return body;
        }
        let limit = request.after_bytes;
        *state = Phase::Taken {
            job_id,
            bytes: 0,
            held: false,
        };
        drop(state);
        Box::pin(Metered {
            body,
            hold: Arc::clone(self),
            job_id,
            limit,
            passed: 0,
            pending: None,
            wait: None,
            released: false,
        })
    }

    fn mark_held(&self, job: Uuid, passed: u64) {
        if let Phase::Taken {
            job_id,
            bytes,
            held,
        } = &mut *self.state()
            && *job_id == job
        {
            *bytes = passed;
            *held = true;
        }
    }

    fn holds(&self, job: Uuid) -> bool {
        matches!(*self.state(), Phase::Taken { job_id, .. } if job_id == job)
    }

    /// Waits until the hold of `job` is released, or until the limit; at the limit the hold is dropped.
    async fn wait(self: Arc<Self>, job: Uuid) {
        let deadline = tokio::time::Instant::now() + self.limit;
        loop {
            let released = self.released.notified();
            tokio::pin!(released);
            // Registered before the check, so a release between the check and the wait is not missed.
            released.as_mut().enable();
            if !self.holds(job) {
                return;
            }
            if tokio::time::timeout_at(deadline, released).await.is_err() {
                if self.holds(job) {
                    *self.state() = Phase::Idle;
                }
                return;
            }
        }
    }
}

type Wait = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A body that passes `limit` bytes, then waits for the release before the rest.
struct Metered {
    body: ByteStream,
    hold: Arc<WorkerHold>,
    job_id: Uuid,
    limit: u64,
    passed: u64,
    pending: Option<Bytes>,
    wait: Option<Wait>,
    released: bool,
}

impl Stream for Metered {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if !this.released && this.passed >= this.limit {
                if this.wait.is_none() {
                    this.hold.mark_held(this.job_id, this.passed);
                    this.wait = Some(Box::pin(Arc::clone(&this.hold).wait(this.job_id)));
                }
                if let Some(wait) = this.wait.as_mut() {
                    ready!(wait.as_mut().poll(cx));
                }
                this.wait = None;
                this.released = true;
            }
            if let Some(rest) = this.pending.take() {
                return Poll::Ready(Some(Ok(rest)));
            }
            let chunk = match ready!(this.body.as_mut().poll_next(cx)) {
                Some(Ok(chunk)) => chunk,
                other => return Poll::Ready(other),
            };
            let len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            if !this.released && this.passed.saturating_add(len) > this.limit {
                let cut = usize::try_from(this.limit - this.passed).unwrap_or(chunk.len());
                this.pending = Some(chunk.slice(cut..));
                this.passed = this.limit;
                if cut == 0 {
                    continue;
                }
                return Poll::Ready(Some(Ok(chunk.slice(..cut))));
            }
            this.passed = this.passed.saturating_add(len);
            return Poll::Ready(Some(Ok(chunk)));
        }
    }
}

#[derive(Clone)]
struct Routes {
    hold: Arc<WorkerHold>,
    clock: Arc<OffsetClock>,
}

/// `POST`, `GET` and `DELETE /test/worker-hold` on the `:9091` listener; no credential, like `POST /test/clock`.
pub(crate) fn router(hold: Arc<WorkerHold>, clock: Arc<OffsetClock>) -> Router {
    Router::new()
        .route("/test/worker-hold", post(arm).get(status).delete(release))
        .with_state(Routes { hold, clock })
}

fn request_id(routes: &Routes, request_id: Option<Extension<CorrelationId>>) -> CorrelationId {
    request_id.map_or_else(
        || CorrelationId::new(sc_ids::new_id(routes.clock.as_ref())),
        |Extension(id)| id,
    )
}

/// `{route: "input" | "output", kind?: "thumb" | "preview" | "transcode", after_bytes}`.
fn hold_request(body: &Value) -> Option<HoldRequest> {
    let route = match body.get("route")?.as_str()? {
        "input" => HoldRoute::Input,
        "output" => HoldRoute::Output,
        _ => return None,
    };
    let kind = match body.get("kind") {
        None | Some(Value::Null) => None,
        Some(kind) => match kind.as_str()? {
            kind @ ("thumb" | "preview" | "transcode") => Some(kind.to_owned()),
            _ => return None,
        },
    };
    let after_bytes = body.get("after_bytes")?.as_u64()?;
    Some(HoldRequest {
        route,
        kind,
        after_bytes,
    })
}

async fn arm(
    State(routes): State<Routes>,
    id: Option<Extension<CorrelationId>>,
    body: Body,
) -> Result<Response, ApiProblem> {
    let request_id = request_id(&routes, id);
    let body = json_body::read(body, MAX_BODY, request_id).await?;
    let request = hold_request(&body).ok_or_else(|| malformed(request_id))?;
    if !routes.hold.arm(request) {
        return Err(ApiProblem::validation("route", "already_armed", request_id));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn status(State(routes): State<Routes>) -> Json<Value> {
    let status = routes.hold.status();
    Json(json!({
        "armed": status.armed,
        "held": status.held,
        "job_id": status.job_id,
        "bytes": status.bytes,
    }))
}

async fn release(State(routes): State<Routes>) -> StatusCode {
    routes.hold.release();
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests;
