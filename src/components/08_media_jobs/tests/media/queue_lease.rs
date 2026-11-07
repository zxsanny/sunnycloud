//! AC-4 (lease expiry and the holder rule) and AC-6 (release and the skipped closes).

use std::time::Duration;

use media_api::{JobQueueError, Priority};
use media_shared::types::{Disposition, JobKind, Lane, SkipOutcome};
use sc_observability::NoLabels;
use uuid::Uuid;

use crate::queue_support::{LEASE, Queue};

const SECOND: Duration = Duration::from_secs(1);
const MICRO: Duration = Duration::from_micros(1);

async fn assert_every_call_loses(q: &Queue, id: Uuid, worker: &str) {
    let extend = q.queue.extend_lease(id, worker).await;
    let release = q.queue.release(id, worker).await;
    let skip = q
        .queue
        .complete_skipped(id, worker, SkipOutcome::Obsolete)
        .await;
    let fail = q
        .queue
        .fail(id, worker, "decode_failed", Disposition::Retry)
        .await;
    assert!(
        matches!(extend, Err(JobQueueError::LeaseLost)),
        "extend {extend:?}"
    );
    assert!(
        matches!(release, Err(JobQueueError::LeaseLost)),
        "release {release:?}"
    );
    assert!(
        matches!(skip, Err(JobQueueError::LeaseLost)),
        "skip {skip:?}"
    );
    assert!(
        matches!(fail, Err(JobQueueError::LeaseLost)),
        "fail {fail:?}"
    );
}

#[tokio::test]
async fn after_the_lease_expires_a_second_worker_gets_the_job_and_the_first_is_refused_everywhere()
{
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    let first = q
        .queue
        .claim(Lane::Image, "w1")
        .await
        .expect("claim")
        .expect("job");
    q.clock.tick(LEASE + SECOND);

    // Act
    let second = q
        .queue
        .claim(Lane::Image, "w2")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    assert_eq!((first.job_id, first.attempt), (id, 1));
    assert_eq!((second.job_id, second.attempt), (id, 2));
    assert_every_call_loses(&q, id, "w1").await;
    assert_every_call_loses(&q, id, "w3").await;
    let extended = q
        .queue
        .extend_lease(id, "w2")
        .await
        .expect("holder extends");
    assert_eq!(extended.lease_ms, LEASE.as_millis() as u64);
    let row = q.row(id).await;
    assert_eq!(
        (row.status.as_str(), row.leased_by.as_deref()),
        ("running", Some("w2"))
    );
    assert_eq!(q.metrics.leases_expired.get(NoLabels), 1);
    q.finish().await;
}

#[tokio::test]
async fn a_lease_is_live_until_its_last_microsecond_and_expired_at_the_instant() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w1")
        .await
        .expect("claim")
        .expect("job");
    q.clock.tick(LEASE - MICRO);

    // Act
    let at_the_edge = q.queue.extend_lease(id, "w1").await;
    q.clock.tick(LEASE);
    let at_expiry = q.queue.extend_lease(id, "w1").await;

    // Assert
    assert!(at_the_edge.is_ok(), "{at_the_edge:?}");
    assert!(matches!(at_expiry, Err(JobQueueError::LeaseLost)));
    let taken = q
        .queue
        .claim(Lane::Image, "w2")
        .await
        .expect("claim")
        .expect("job");
    assert_eq!((taken.job_id, taken.attempt), (id, 2));
    q.finish().await;
}

#[tokio::test]
async fn a_heartbeat_moves_the_lease_so_the_job_is_not_claimable_meanwhile() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w1")
        .await
        .expect("claim")
        .expect("job");

    // Act
    q.clock.tick(LEASE - SECOND);
    q.queue.extend_lease(id, "w1").await.expect("heartbeat");
    q.clock.tick(LEASE - SECOND);
    let rival = q.queue.claim(Lane::Image, "w2").await.expect("claim");
    let still_held = q.queue.extend_lease(id, "w1").await;

    // Assert
    assert!(rival.is_none());
    assert!(still_held.is_ok());
    q.finish().await;
}

#[tokio::test]
async fn only_a_running_job_leased_by_the_caller_passes_the_holder_check() {
    // Arrange
    let q = Queue::new().await;
    let queued = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    let closed = q.add(JobKind::Poster, 2, Priority::Interactive).await;
    q.clock.tick(SECOND);
    let held = q.add(JobKind::Transcode, 3, Priority::Interactive).await;
    q.queue
        .claim(Lane::Video, "w1")
        .await
        .expect("claim")
        .expect("job");
    q.queue
        .complete_skipped(closed, "w1", SkipOutcome::SkippedCompliant)
        .await
        .expect("close");
    q.queue
        .claim(Lane::Video, "w1")
        .await
        .expect("claim")
        .expect("job");

    // Act and Assert
    assert_every_call_loses(&q, queued, "w1").await;
    assert_every_call_loses(&q, closed, "w1").await;
    assert_every_call_loses(&q, held, "w2").await;
    assert_every_call_loses(&q, Uuid::new_v4(), "w1").await;
    assert_eq!(q.row(held).await.status, "running");
    assert_eq!(q.row(queued).await.status, "queued");
    q.finish().await;
}

#[tokio::test]
async fn release_requeues_with_the_attempt_back_and_skipped_closes_have_no_derived_row() {
    // Arrange
    let q = Queue::new().await;
    let j1 = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    let j2 = q.add(JobKind::Stills, 2, Priority::Interactive).await;
    let j3 = q.add(JobKind::Stills, 3, Priority::Interactive).await;
    for _ in 0..3 {
        q.queue
            .claim(Lane::Image, "w1")
            .await
            .expect("claim")
            .expect("job");
    }
    q.clock.tick(SECOND);

    // Act
    q.queue.release(j1, "w1").await.expect("release");
    q.queue
        .complete_skipped(j2, "w1", SkipOutcome::SkippedCompliant)
        .await
        .expect("skip");
    q.queue
        .complete_skipped(j3, "w1", SkipOutcome::Obsolete)
        .await
        .expect("obsolete");
    let again = q
        .queue
        .claim(Lane::Image, "w2")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    let (r1, r2, r3) = (q.row(j1).await, q.row(j2).await, q.row(j3).await);
    assert_eq!(again.job_id, j1);
    assert_eq!(again.attempt, 1);
    assert_eq!(r1.leased_by.as_deref(), Some("w2"));
    assert_eq!(
        (r2.status.as_str(), r2.outcome.as_deref()),
        ("succeeded", Some("skipped_compliant"))
    );
    assert_eq!(
        (r3.status.as_str(), r3.outcome.as_deref()),
        ("succeeded", Some("obsolete"))
    );
    assert_eq!(r2.finished_at, Some(crate::queue_support::micros(&q.clock)));
    assert_eq!(q.count("SELECT count(*) FROM derived").await, 0);
    q.finish().await;
}

#[tokio::test]
async fn a_released_job_is_queued_with_zero_attempts_and_claimable_at_once() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w1")
        .await
        .expect("claim")
        .expect("job");

    // Act
    q.queue.release(id, "w1").await.expect("release");

    // Assert
    let row = q.row(id).await;
    assert_eq!(
        (row.status.as_str(), row.attempts, row.leased_by),
        ("queued", 0, None)
    );
    assert_eq!(row.available_at, crate::queue_support::micros(&q.clock));
    assert!(
        q.queue
            .claim(Lane::Image, "w2")
            .await
            .expect("claim")
            .is_some()
    );
    q.finish().await;
}

#[tokio::test]
async fn a_non_holder_cannot_close_or_release_the_job_of_another_worker() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w1")
        .await
        .expect("claim")
        .expect("job");

    // Act
    let release = q.queue.release(id, "intruder").await;
    let skip = q
        .queue
        .complete_skipped(id, "intruder", SkipOutcome::Obsolete)
        .await;

    // Assert
    assert!(matches!(release, Err(JobQueueError::LeaseLost)));
    assert!(matches!(skip, Err(JobQueueError::LeaseLost)));
    let row = q.row(id).await;
    assert_eq!(
        (row.status.as_str(), row.attempts, row.leased_by.as_deref()),
        ("running", 1, Some("w1"))
    );
    q.finish().await;
}
