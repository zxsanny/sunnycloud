//! AC-4 and AC-6 of AZ-3967: `cancel()` stops everything within 1 s; a disabled account stops all work.

mod request_support;
mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use request_support::{FixtureTransport, SentinelCredentials, clock, eventually, get};
use sc_error::ErrorCode;
use serde_json::json;
use support::{Answer, Fixture, Route, Script};
use sync_core::{Mapped, RequestPath, SyncError, WireResponse};
use sync_core_proto::RetryPolicy;

const TARGET: &str = "/api/v1/nodes/root";

fn policy() -> RetryPolicy {
    RetryPolicy {
        base: Duration::from_millis(10),
        max: Duration::from_millis(100),
        max_attempts_per_block: 8,
    }
}

fn path_for(fixture: &Fixture) -> Arc<RequestPath> {
    let clock = clock();
    Arc::new(RequestPath::new(
        FixtureTransport::new(fixture, clock.clone()),
        SentinelCredentials::new(),
        clock,
        policy(),
    ))
}

fn cancelled(result: &Result<WireResponse, Mapped>) -> bool {
    matches!(result, Err(Mapped::Error(SyncError::Cancelled)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_ends_three_requests_in_flight_within_a_second_and_nothing_is_sent_after() {
    let script = Script::new().always(
        Route::RootNode,
        Answer::json(200, &json!({})).delay(Duration::from_secs(30)),
    );
    let fixture = Fixture::start(script).await;
    let path = path_for(&fixture);
    let mut calls = Vec::new();
    for _ in 0..3 {
        let path = path.clone();
        calls.push(tokio::spawn(async move {
            path.send("test", get(TARGET, 1)).await
        }));
    }
    eventually(5_000, "the fake to receive all three requests", || {
        fixture.requests().len() == 3
    })
    .await;

    let started = Instant::now();
    path.cancel();
    let mut results = Vec::new();
    for call in calls {
        results.push(call.await.expect("request task"));
    }
    let fourth = path.send("test", get(TARGET, 1)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(results.iter().all(cancelled), "{results:?}");
    assert!(cancelled(&fourth));
    assert_eq!(path.check(), Err(SyncError::Cancelled));
    assert_eq!(fixture.requests().len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_also_ends_a_wait_between_retries() {
    let script = Script::new().always(
        Route::RootNode,
        Answer::problem(ErrorCode::RateLimited).retry_after(3),
    );
    let fixture = Fixture::start(script).await;
    let path = path_for(&fixture);
    let waiting = {
        let path = path.clone();
        tokio::spawn(async move { path.send("test", get(TARGET, 3)).await })
    };
    eventually(5_000, "the first 429", || {
        fixture
            .requests()
            .first()
            .is_some_and(|r| r.answered == Some(429))
    })
    .await;

    path.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .expect("the retry wait ends within a second")
        .expect("request task");

    assert!(cancelled(&result), "{result:?}");
    assert_eq!(fixture.requests().len(), 1);
}

#[tokio::test]
async fn a_disabled_account_stops_every_later_request() {
    let script = Script::new()
        .always(
            Route::RootNode,
            Answer::problem(ErrorCode::AccountNotActive),
        )
        .always(Route::ServerInfo, Answer::json(200, &json!({})));
    let fixture = Fixture::start(script).await;
    let path = path_for(&fixture);

    let first = path.send("test", get(TARGET, 3)).await;
    let second = path.send("test", get("/api/v1/server-info", 3)).await;
    let third = path.send("test", get(TARGET, 3)).await;

    let stopped = Err(Mapped::Error(SyncError::AccountNotActive));
    assert_eq!(first, stopped);
    assert_eq!(second, stopped);
    assert_eq!(third, stopped);
    assert_eq!(path.check(), Err(SyncError::AccountNotActive));
    assert_eq!(fixture.requests().len(), 1);
}
