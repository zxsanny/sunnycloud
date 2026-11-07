//! AC-5: failure reports apply the backoff on the API clock, the dead rule and the error bound.

use std::time::Duration;

use media_api::Priority;
use media_shared::types::{Disposition, FailResult, JobKind, Lane};
use sc_observability::NoLabels;

use crate::queue_support::{ATTEMPTS, BACKOFF, Queue, micros};

const MILLI: Duration = Duration::from_millis(1);

#[tokio::test]
async fn a_retry_waits_the_backoff_and_the_backoff_doubles_with_the_attempt() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");

    // Act
    let first = q
        .queue
        .fail(id, "w", "decode_failed", Disposition::Retry)
        .await
        .expect("fail");
    let first_row = q.row(id).await;
    let first_due = micros(&q.clock) + BACKOFF.as_micros() as i64;
    q.clock.tick(BACKOFF - MILLI);
    let early = q.queue.claim(Lane::Image, "w").await.expect("claim");
    q.clock.tick(MILLI);
    let second_claim = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    let second = q
        .queue
        .fail(id, "w", "decode_failed", Disposition::Retry)
        .await
        .expect("fail");
    let second_row = q.row(id).await;
    let second_due = micros(&q.clock) + 2 * BACKOFF.as_micros() as i64;
    q.clock.tick(2 * BACKOFF - MILLI);
    let early_again = q.queue.claim(Lane::Image, "w").await.expect("claim");
    q.clock.tick(MILLI);
    let third_claim = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    assert_eq!([first, second], [FailResult::Requeued; 2]);
    assert_eq!(
        (first_row.status.as_str(), first_row.available_at),
        ("queued", first_due)
    );
    assert_eq!(
        (second_row.status.as_str(), second_row.available_at),
        ("queued", second_due)
    );
    assert_eq!(first_row.leased_by, None);
    assert_eq!(first_row.last_error.as_deref(), Some("decode_failed"));
    assert!(early.is_none() && early_again.is_none());
    assert_eq!((second_claim.attempt, third_claim.attempt), (2, 3));
    assert_eq!(q.metrics.dead.get(NoLabels), 0);
    q.finish().await;
}

#[tokio::test]
async fn the_last_attempt_and_a_permanent_failure_are_dead_and_counted() {
    // Arrange
    let q = Queue::new().await;
    let retried = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    let mut results = Vec::new();

    // Act
    for attempt in 1..=ATTEMPTS {
        q.queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .expect("job");
        results.push(
            q.queue
                .fail(retried, "w", "oom", Disposition::Retry)
                .await
                .expect("fail"),
        );
        q.clock.tick(BACKOFF * 2u32.pow(attempt as u32));
    }
    let permanent = q.add(JobKind::Stills, 2, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    let permanent_result = q
        .queue
        .fail(permanent, "w", "corrupt", Disposition::Permanent)
        .await
        .expect("fail");

    // Assert
    assert_eq!(
        results,
        [FailResult::Requeued, FailResult::Requeued, FailResult::Dead]
    );
    assert_eq!(permanent_result, FailResult::Dead);
    let (dead_retried, dead_permanent) = (q.row(retried).await, q.row(permanent).await);
    assert_eq!(
        (dead_retried.status.as_str(), dead_retried.attempts),
        ("dead", 3)
    );
    assert_eq!(
        (dead_permanent.status.as_str(), dead_permanent.attempts),
        ("dead", 1)
    );
    assert_eq!(dead_permanent.finished_at, Some(micros(&q.clock)));
    assert_eq!(dead_permanent.last_error.as_deref(), Some("corrupt"));
    assert_eq!(q.metrics.dead.get(NoLabels), 2);
    assert!(
        q.queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .is_none()
    );
    q.finish().await;
}

#[tokio::test]
async fn the_stored_error_is_at_most_500_characters() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    let long = "é".repeat(2_000);

    // Act
    q.queue
        .fail(id, "w", &long, Disposition::Retry)
        .await
        .expect("fail");

    // Assert
    let stored = q.row(id).await.last_error.expect("stored error");
    assert_eq!(stored.chars().count(), 500);
    q.finish().await;
}

#[tokio::test]
async fn an_error_with_a_nul_character_is_stored_without_it() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");

    // Act
    q.queue
        .fail(id, "w", "bad\0code", Disposition::Permanent)
        .await
        .expect("fail");

    // Assert
    assert_eq!(q.row(id).await.last_error.as_deref(), Some("badcode"));
    q.finish().await;
}
