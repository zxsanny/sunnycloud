//! AC-5 of AZ-3967: `Retry-After` is honoured on the injected clock, with jitter under one second.

mod request_support;
mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use request_support::{FixtureTransport, SentinelCredentials, clock, eventually, get};
use sc_clock::ManualClock;
use sc_error::{ErrorCode, TransportKind};
use serde_json::json;
use support::{Answer, Fault, Fixture, Route, Script};
use sync_core::{Mapped, RequestPath, SyncError};
use sync_core_proto::RetryPolicy;

const TARGET: &str = "/api/v1/nodes/root";

fn policy() -> RetryPolicy {
    RetryPolicy {
        base: Duration::from_millis(500),
        max: Duration::from_secs(30),
        max_attempts_per_block: 8,
    }
}

struct Rig {
    fixture: Fixture,
    clock: Arc<ManualClock>,
    transport: Arc<FixtureTransport>,
    credentials: Arc<SentinelCredentials>,
    path: Arc<RequestPath>,
}

async fn rig(script: Script) -> Rig {
    let fixture = Fixture::start(script).await;
    let clock = clock();
    let transport = FixtureTransport::new(&fixture, clock.clone());
    let credentials = SentinelCredentials::new();
    let path = Arc::new(RequestPath::new(
        transport.clone(),
        credentials.clone(),
        clock.clone(),
        policy(),
    ));
    Rig {
        fixture,
        clock,
        transport,
        credentials,
        path,
    }
}

/// Moves the clock forward in `step`s until `done`, at most `limit` in total; real time passes between steps
/// only so the request task can run.
async fn tick_until(rig: &Rig, step: Duration, limit: Duration, done: impl Fn() -> bool) {
    let mut moved = Duration::ZERO;
    while !done() && moved < limit {
        rig.clock.tick(step);
        moved += step;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_next_attempt_comes_three_to_four_seconds_after_a_429_with_retry_after_3() {
    let script = Script::new()
        .on(
            Route::RootNode,
            Answer::problem(ErrorCode::RateLimited).retry_after(3),
        )
        .always(Route::RootNode, Answer::json(200, &json!({})));
    let rig = rig(script).await;
    let call = {
        let path = rig.path.clone();
        tokio::spawn(async move { path.send("test", get(TARGET, 3)).await })
    };
    eventually(5_000, "the first 429", || {
        rig.fixture
            .requests()
            .first()
            .is_some_and(|r| r.answered == Some(429))
    })
    .await;

    tick_until(
        &rig,
        Duration::from_millis(100),
        Duration::from_secs(10),
        || rig.transport.sent_at().len() >= 2,
    )
    .await;
    let response = call
        .await
        .expect("request task")
        .expect("second attempt succeeds");

    let sent = rig.transport.sent_at();
    let waited = sent[1].saturating_duration_since(sent[0]);
    assert_eq!(response.status, 200);
    assert_eq!(sent.len(), 2);
    assert!(waited >= Duration::from_secs(3), "retried after {waited:?}");
    assert!(waited <= Duration::from_secs(4), "retried after {waited:?}");
}

#[tokio::test]
async fn without_retries_a_429_surfaces_with_its_delay() {
    let script = Script::new().always(
        Route::RootNode,
        Answer::problem(ErrorCode::RateLimited).retry_after(3),
    );
    let rig = rig(script).await;

    let result = rig.path.send("test", get(TARGET, 1)).await;

    assert_eq!(
        result,
        Err(Mapped::Error(SyncError::RateLimited {
            retry_after: Some(Duration::from_secs(3))
        }))
    );
    assert_eq!(rig.fixture.requests().len(), 1);
}

#[tokio::test]
async fn a_429_that_never_clears_ends_after_the_allowed_attempts() {
    let script = Script::new().always(
        Route::RootNode,
        Answer::problem(ErrorCode::RateLimited).retry_after(1),
    );
    let rig = rig(script).await;
    let call = {
        let path = rig.path.clone();
        tokio::spawn(async move { path.send("test", get(TARGET, 2)).await })
    };

    tick_until(
        &rig,
        Duration::from_millis(100),
        Duration::from_secs(10),
        || call.is_finished(),
    )
    .await;
    let result = call.await.expect("request task");

    assert!(matches!(
        result,
        Err(Mapped::Error(SyncError::RateLimited { .. }))
    ));
    assert_eq!(rig.fixture.requests().len(), 2);
}

#[tokio::test]
async fn a_lost_connection_is_retried_with_a_fresh_credential_read() {
    let script = Script::new()
        .on(
            Route::RootNode,
            Answer::json(200, &json!({})).fault(Fault::DropConnection),
        )
        .always(Route::RootNode, Answer::json(200, &json!({})));
    let rig = rig(script).await;
    let call = {
        let path = rig.path.clone();
        tokio::spawn(async move { path.send("test", get(TARGET, 3)).await })
    };

    tick_until(
        &rig,
        Duration::from_millis(100),
        Duration::from_secs(10),
        || call.is_finished(),
    )
    .await;
    let result = call.await.expect("request task");

    assert_eq!(result.expect("second attempt succeeds").status, 200);
    assert_eq!(rig.fixture.requests().len(), 2);
    assert_eq!(rig.credentials.bearer_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_transport_failure_without_retries_is_reported_as_transport() {
    let script = Script::new().always(
        Route::RootNode,
        Answer::json(200, &json!({})).fault(Fault::DropConnection),
    );
    let rig = rig(script).await;

    let result = rig.path.send("test", get(TARGET, 1)).await;

    assert!(
        matches!(
            result,
            Err(Mapped::Error(SyncError::Transport(
                TransportKind::Other | TransportKind::Connect
            )))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_definite_answer_is_not_retried() {
    let script = Script::new().always(Route::RootNode, Answer::problem(ErrorCode::NotFound));
    let rig = rig(script).await;

    let result = rig.path.send("test", get(TARGET, 5)).await;

    assert_eq!(result, Err(Mapped::Error(SyncError::NotFound)));
    assert_eq!(rig.fixture.requests().len(), 1);
}
