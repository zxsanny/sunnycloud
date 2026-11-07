//! AC-2 (order and lanes), AC-3 (no double claim), AC-7 (an exhausted expired job is never handed out) and the
//! `SKIP LOCKED` rule, on a real PostgreSQL 18.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use media_api::Priority;
use media_shared::types::{JobKind, Lane};
use sqlx::Connection;
use tokio::sync::Barrier;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::queue_support::{ATTEMPTS, LEASE, Queue, micros};

const SECOND: Duration = Duration::from_secs(1);

#[tokio::test]
async fn the_interactive_job_comes_first_then_bulk_in_due_order_and_lanes_stay_apart() {
    // Arrange
    let q = Queue::new().await;
    let mut bulk = Vec::new();
    for n in 0..10 {
        bulk.push(q.add(JobKind::Stills, n, Priority::Bulk).await);
        q.clock.tick(SECOND);
    }
    let interactive = q.add(JobKind::Stills, 100, Priority::Interactive).await;
    q.clock.tick(SECOND);
    let mut video = HashSet::new();
    for (n, kind) in [
        (200, JobKind::Transcode),
        (201, JobKind::Poster),
        (202, JobKind::Transcode),
        (203, JobKind::Poster),
        (204, JobKind::Transcode),
    ] {
        video.insert(q.add(kind, n, Priority::Interactive).await);
        q.clock.tick(SECOND);
    }

    // Act
    let mut images = Vec::new();
    while let Some(job) = q.queue.claim(Lane::Image, "w1").await.expect("claim image") {
        images.push(job);
    }
    let claimed_at = micros(&q.clock);
    let mut videos = Vec::new();
    while let Some(job) = q.queue.claim(Lane::Video, "w2").await.expect("claim video") {
        videos.push(job);
    }

    // Assert
    let expected: Vec<Uuid> = std::iter::once(interactive).chain(bulk).collect();
    assert_eq!(
        images.iter().map(|job| job.job_id).collect::<Vec<_>>(),
        expected
    );
    assert!(images.iter().all(|job| job.kind == JobKind::Stills));
    assert_eq!(
        videos.iter().map(|job| job.job_id).collect::<HashSet<_>>(),
        video
    );
    assert!(videos.iter().all(|job| job.kind != JobKind::Stills));
    for (job, worker) in images
        .iter()
        .map(|j| (j, "w1"))
        .chain(videos.iter().map(|j| (j, "w2")))
    {
        let row = q.row(job.job_id).await;
        assert_eq!((row.status.as_str(), row.attempts), ("running", 1));
        assert_eq!(row.leased_by.as_deref(), Some(worker));
        assert_eq!(job.attempt, 1);
        assert_eq!(job.lease_ms, LEASE.as_millis() as u64);
        assert_eq!(job.params_version, 1);
        assert_eq!(row.available_at, claimed_at + LEASE.as_micros() as i64);
    }
    q.finish().await;
}

#[tokio::test]
async fn one_claim_crosses_accounts() {
    // Arrange
    let q = Queue::new().await;
    let (other_account, other_revision) = q.second_account().await;
    let first = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.clock.tick(SECOND);
    let foreign_spec = q.spec_of(other_account, other_revision, JobKind::Stills, 2);
    q.enqueue(foreign_spec, Priority::Interactive).await;
    let foreign = q.id_of(&foreign_spec).await;

    // Act
    let a = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    let b = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    assert_eq!([a.job_id, b.job_id], [first, foreign]);
    q.finish().await;
}

#[tokio::test]
async fn twenty_concurrent_claimers_take_500_jobs_exactly_once() {
    // Arrange
    let q = Queue::new().await;
    q.add_many(JobKind::Stills, 0, 500, Priority::Bulk).await;
    let start = Arc::new(Barrier::new(20));
    let mut claimers = JoinSet::new();

    // Act
    for claimer in 0..20 {
        let (queue, start) = (q.queue.clone(), start.clone());
        claimers.spawn(async move {
            let worker = format!("worker-{claimer}");
            start.wait().await;
            let mut taken = Vec::new();
            while let Some(job) = queue.claim(Lane::Image, &worker).await.expect("claim") {
                taken.push((job.job_id, worker.clone(), job.attempt));
            }
            taken
        });
    }
    let mut claimed = Vec::new();
    while let Some(taken) = claimers.join_next().await {
        claimed.extend(taken.expect("claimer task"));
    }

    // Assert
    let distinct: HashSet<Uuid> = claimed.iter().map(|(id, ..)| *id).collect();
    assert_eq!(claimed.len(), 500);
    assert_eq!(distinct.len(), 500, "a job was handed out twice");
    assert!(claimed.iter().all(|(.., attempt)| *attempt == 1));
    assert_eq!(
        q.count("SELECT count(*) FROM jobs WHERE status = 'running' AND leased_by IS NOT NULL")
            .await,
        500
    );
    let holders = q.count("SELECT count(DISTINCT leased_by) FROM jobs").await;
    assert!((1..=20).contains(&holders));
    q.finish().await;
}

#[tokio::test]
async fn a_claim_skips_a_row_locked_by_another_transaction_instead_of_waiting() {
    // Arrange
    let q = Queue::new().await;
    let first = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    q.clock.tick(SECOND);
    let second = q.add(JobKind::Stills, 2, Priority::Interactive).await;
    q.clock.tick(SECOND);
    let mut holder = q.fixture.owner_conn().await;
    sqlx::query("BEGIN")
        .execute(&mut holder)
        .await
        .expect("begin");
    sqlx::query("SELECT id FROM jobs WHERE id = $1 FOR UPDATE")
        .bind(first)
        .execute(&mut holder)
        .await
        .expect("lock the first job");

    // Act
    let while_locked = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    let none_left = q.queue.claim(Lane::Image, "w").await.expect("claim");
    sqlx::query("ROLLBACK")
        .execute(&mut holder)
        .await
        .expect("rollback");
    holder.close().await.expect("close holder");
    let after = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    assert_eq!(while_locked.job_id, second);
    assert!(none_left.is_none());
    assert_eq!(after.job_id, first);
    q.finish().await;
}

#[tokio::test]
async fn an_exhausted_job_with_an_expired_lease_is_not_handed_out_and_is_marked_dead() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    for attempt in 1..=ATTEMPTS {
        let job = q
            .queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .expect("job");
        assert_eq!((job.job_id, u64::from(job.attempt)), (id, attempt));
        q.clock.tick(LEASE + SECOND);
    }

    // Act
    let fourth = q.queue.claim(Lane::Image, "w2").await.expect("claim");

    // Assert
    assert!(fourth.is_none());
    let row = q.row(id).await;
    assert_eq!((row.status.as_str(), row.attempts), ("dead", 3));
    assert_eq!(row.finished_at, Some(micros(&q.clock)));
    assert_eq!(row.last_error.as_deref(), Some("lease_expired"));
    assert_eq!(q.metrics.dead.get(sc_observability::NoLabels), 1);
    assert_eq!(q.metrics.leases_expired.get(sc_observability::NoLabels), 3);
    q.finish().await;
}

#[tokio::test]
async fn the_claim_closes_an_exhausted_job_it_meets_and_still_returns_the_next_job() {
    // Arrange
    let q = Queue::new().await;
    let doomed = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    for _ in 0..ATTEMPTS {
        q.queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .expect("job");
        q.clock.tick(LEASE + SECOND);
    }
    let waiting = q.add(JobKind::Stills, 2, Priority::Interactive).await;

    // Act
    let next = q
        .queue
        .claim(Lane::Image, "w2")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    assert_eq!(next.job_id, waiting);
    assert_eq!(q.row(doomed).await.status, "dead");
    q.finish().await;
}
