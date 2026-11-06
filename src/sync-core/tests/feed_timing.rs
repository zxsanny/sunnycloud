//! AC-4 and AC-5 of AZ-3965: the long-poll asks for the server's clamped wait and gives up at wait plus margin
//! with a retryable transport error; failures back off from `retry_base` to `retry_max` and honour `Retry-After`.

mod connect_support;
mod feed_support;
mod request_support;
mod support;
mod tree_support;

use std::time::{Duration, Instant};

use feed_support::{connected_with, item, ms, page, page_answer, target_param};
use sc_error::{ErrorCode, TransportKind};
use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncConfig, SyncError};
use sync_core_proto::Cursor;
use tree_support::seen;

const START: Cursor = Cursor::new(1, 100);

#[tokio::test]
async fn a_silent_server_is_given_up_on_at_the_clamped_wait_plus_the_margin_and_the_call_can_be_retried()
 {
    // Arrange
    let cap_seconds = 1;
    let margin = ms(200);
    let good = page(vec![item(101, 1)], Cursor::new(1, 101), false);
    let script = Script::new()
        .on(
            Route::Changes,
            page_answer(&page(Vec::new(), START, false)).delay(Duration::from_secs(10)),
        )
        .on(Route::Changes, page_answer(&good));
    let (api, fixture) = connected_with(script, cap_seconds, |config| SyncConfig {
        longpoll_client_margin: margin,
        request_idle_timeout: Duration::from_secs(5),
        ..config
    })
    .await;

    // Act
    let started = Instant::now();
    let silent = api.poll_changes(START, Duration::from_secs(30), None).await;
    let waited = started.elapsed();
    let retried = api.poll_changes(START, Duration::ZERO, None).await;

    // Assert
    assert_eq!(silent, Err(SyncError::Transport(TransportKind::Timeout)));
    assert!(
        waited >= Duration::from_secs(cap_seconds) + margin,
        "gave up after {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(cap_seconds) + margin + ms(800),
        "gave up after {waited:?}"
    );
    assert_eq!(retried, Ok(good));
    let first = &seen(&fixture, Route::Changes)[0];
    assert_eq!(target_param(&first.target, "wait").as_deref(), Some("1"));
}

#[tokio::test]
async fn failures_back_off_from_the_base_to_the_maximum_and_a_429_is_waited_out_in_full() {
    // Arrange
    let good = page(vec![item(101, 1)], Cursor::new(1, 101), false);
    let mut script = Script::new().on_times(
        Route::Changes,
        Answer::problem(ErrorCode::ServiceUnavailable),
        5,
    );
    script = script
        .on(
            Route::Changes,
            Answer::problem(ErrorCode::RateLimited).retry_after(3),
        )
        .on(Route::Changes, page_answer(&good));
    let (api, _fixture) = connected_with(script, 30, |config| SyncConfig {
        retry_base: ms(40),
        retry_max: ms(160),
        ..config
    })
    .await;

    // Act
    let mut results = Vec::new();
    let mut gaps = Vec::new();
    let mut last = Instant::now();
    for _ in 0..7 {
        results.push(api.poll_changes(START, ms(0), None).await);
        gaps.push(last.elapsed());
        last = Instant::now();
    }

    // Assert
    for result in &results[..5] {
        assert!(
            matches!(result, Err(SyncError::Unexpected { .. })),
            "{result:?}"
        );
    }
    assert_eq!(
        results[5],
        Err(SyncError::RateLimited {
            retry_after: Some(Duration::from_secs(3))
        })
    );
    assert_eq!(results[6], Ok(good));
    // Gap n holds the rest after failure n - 1 plus one round trip. The jitter is below the base (40 ms).
    let rests = [(0, 0), (40, 80), (80, 120), (160, 200), (160, 200)];
    for (index, (low, high)) in rests.iter().enumerate() {
        let gap = gaps[index].as_millis();
        assert!(
            gap >= *low && gap < high + 250,
            "gap {index} was {gap} ms, rest {low}..{high} ms"
        );
    }
    let after_429 = gaps[6];
    assert!(after_429 >= Duration::from_secs(3), "{after_429:?}");
    assert!(
        after_429 < Duration::from_secs(3) + ms(40 + 250),
        "{after_429:?}"
    );
}
