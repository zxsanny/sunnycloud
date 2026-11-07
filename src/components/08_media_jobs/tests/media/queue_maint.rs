//! AC-8: reap, purge of terminal rows and requeue of dead jobs.

use std::time::Duration;

use media_api::Priority;
use media_shared::types::{Disposition, JobKind, Lane, SkipOutcome};
use sc_observability::NoLabels;
use uuid::Uuid;

use crate::queue_support::{LEASE, Queue, config};

const SECOND: Duration = Duration::from_secs(1);

/// Adds a job, leases it (it is the only due job of its lane) and closes it as skipped or dead.
async fn finished(q: &Queue, n: u32, dead: bool) -> Uuid {
    let id = q.add(JobKind::Stills, n, Priority::Interactive).await;
    let job = q
        .queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(job.job_id, id);
    if dead {
        q.queue
            .fail(id, "w", "corrupt", Disposition::Permanent)
            .await
            .expect("fail");
    } else {
        q.queue
            .complete_skipped(id, "w", SkipOutcome::SkippedCompliant)
            .await
            .expect("skip");
    }
    id
}

#[tokio::test]
async fn reap_marks_only_expired_jobs_with_exhausted_attempts_dead() {
    // Arrange
    let q = Queue::new().await;
    let e1 = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    let e2 = q.add(JobKind::Stills, 2, Priority::Interactive).await;
    let spared = q.add(JobKind::Poster, 3, Priority::Interactive).await;
    q.queue
        .claim(Lane::Video, "w")
        .await
        .expect("claim")
        .expect("job");
    for _ in 0..3 {
        q.queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .expect("job");
        q.queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .expect("job");
        q.clock.tick(LEASE + SECOND);
    }
    let (expired_before, dead_before) = (
        q.metrics.leases_expired.get(NoLabels),
        q.metrics.dead.get(NoLabels),
    );

    // Act
    let stats = q.queue.reap(q.clock_now()).await.expect("reap");
    let again = q.queue.reap(q.clock_now()).await.expect("reap again");

    // Assert
    assert_eq!((stats.dead, again.dead), (2, 0));
    for id in [e1, e2] {
        let row = q.row(id).await;
        assert_eq!((row.status.as_str(), row.attempts), ("dead", 3));
        assert_eq!(
            row.finished_at,
            Some(crate::queue_support::micros(&q.clock))
        );
    }
    let row = q.row(spared).await;
    assert_eq!((row.status.as_str(), row.attempts), ("running", 1));
    assert_eq!(q.metrics.leases_expired.get(NoLabels) - expired_before, 2);
    assert_eq!(q.metrics.dead.get(NoLabels) - dead_before, 2);
    q.finish().await;
}

#[tokio::test]
async fn reap_leaves_an_exhausted_job_whose_lease_is_still_live_until_it_expires() {
    // Arrange
    let q = Queue::new().await;
    let id = q.add(JobKind::Stills, 1, Priority::Interactive).await;
    for round in 1..=3 {
        q.queue
            .claim(Lane::Image, "w")
            .await
            .expect("claim")
            .expect("job");
        if round < 3 {
            q.clock.tick(LEASE + SECOND);
        }
    }

    // Act
    let live = q.queue.reap(q.clock_now()).await.expect("reap");
    let live_row = q.row(id).await;
    q.clock.tick(LEASE);
    let expired = q.queue.reap(q.clock_now()).await.expect("reap");

    // Assert
    assert_eq!(live.dead, 0);
    assert_eq!(
        (live_row.status.as_str(), live_row.attempts),
        ("running", 3)
    );
    assert_eq!(expired.dead, 1);
    assert_eq!(q.row(id).await.status, "dead");
    q.finish().await;
}

#[tokio::test]
async fn purge_deletes_terminal_rows_older_than_the_ttl_and_nothing_else() {
    // Arrange
    let q = Queue::new().await;
    let ttl = config().job_terminal_ttl;
    let old_skipped = finished(&q, 1, false).await;
    let old_dead = finished(&q, 2, true).await;
    q.clock.tick(SECOND);
    let boundary = finished(&q, 3, false).await;
    q.clock.tick(ttl);
    let recent = finished(&q, 4, false).await;
    let queued = q.add(JobKind::Stills, 5, Priority::Interactive).await;
    let running = q.add(JobKind::Poster, 6, Priority::Interactive).await;
    q.queue
        .claim(Lane::Video, "w")
        .await
        .expect("claim")
        .expect("job");
    let cutoff = q.clock_now().checked_sub(ttl).expect("cutoff");

    // Act
    let deleted = q.queue.purge_terminal(cutoff).await.expect("purge");

    // Assert
    assert_eq!(deleted, 2);
    let left: Vec<Uuid> = {
        let mut conn = q.fixture.owner_conn().await;
        let ids = sqlx::query_scalar("SELECT id FROM jobs")
            .fetch_all(&mut conn)
            .await
            .expect("ids");
        sqlx::Connection::close(conn).await.expect("close");
        ids
    };
    for gone in [old_skipped, old_dead] {
        assert!(!left.contains(&gone));
    }
    for kept in [boundary, recent, queued, running] {
        assert!(left.contains(&kept));
    }
    q.finish().await;
}

#[tokio::test]
async fn purge_works_in_batches_and_never_deletes_a_terminal_row_without_finished_at() {
    // Arrange
    let q = Queue::new().await;
    let mut conn = q.fixture.owner_conn().await;
    sqlx::query(
        "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
         available_at, created_at, finished_at, outcome) \
         SELECT gen_random_uuid(), $1, $2, 'stills', sha256(convert_to(n::text, 'UTF8')), 1, 0, 'succeeded', \
         'epoch'::timestamptz, 'epoch'::timestamptz, \
         CASE WHEN n = 0 THEN NULL ELSE 'epoch'::timestamptz END, 'skipped_compliant' \
         FROM generate_series(0, 2500) AS n",
    )
    .bind(q.account)
    .bind(q.revision)
    .execute(&mut conn)
    .await
    .expect("insert terminal rows");
    sqlx::Connection::close(conn).await.expect("close");

    // Act
    let deleted = q.queue.purge_terminal(q.clock_now()).await.expect("purge");

    // Assert
    assert_eq!(deleted, 2_500);
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 1);
    q.finish().await;
}

#[tokio::test]
async fn requeue_dead_by_account_by_kind_and_all_resets_the_matching_dead_jobs() {
    // Arrange
    let q = Queue::new().await;
    let (account_b, revision_b) = q.second_account().await;
    let mut dead = Vec::new();
    for (account, revision, kind, lane, n) in [
        (q.account, q.revision, JobKind::Stills, Lane::Image, 1),
        (q.account, q.revision, JobKind::Transcode, Lane::Video, 2),
        (account_b, revision_b, JobKind::Stills, Lane::Image, 3),
        (account_b, revision_b, JobKind::Transcode, Lane::Video, 4),
    ] {
        let spec = q.spec_of(account, revision, kind, n);
        q.enqueue(spec, Priority::Interactive).await;
        let job = q.queue.claim(lane, "w").await.expect("claim").expect("job");
        q.queue
            .fail(job.job_id, "w", "corrupt", Disposition::Permanent)
            .await
            .expect("fail");
        dead.push(job.job_id);
    }
    let healthy = q.add(JobKind::Poster, 9, Priority::Interactive).await;

    // Act
    let by_account = q
        .queue
        .requeue_dead(Some(q.account), None)
        .await
        .expect("by account");
    let by_kind = q
        .queue
        .requeue_dead(None, Some(JobKind::Transcode))
        .await
        .expect("by kind");
    let nothing = q
        .queue
        .requeue_dead(Some(Uuid::new_v4()), None)
        .await
        .expect("no match");
    let rest = q.queue.requeue_dead(None, None).await.expect("all");

    // Assert
    assert_eq!([by_account, by_kind, nothing, rest], [2, 1, 0, 1]);
    for id in dead {
        let row = q.row(id).await;
        assert_eq!(
            (row.status.as_str(), row.attempts, row.leased_by),
            ("queued", 0, None)
        );
        assert_eq!(row.available_at, crate::queue_support::micros(&q.clock));
    }
    assert_eq!(q.row(healthy).await.status, "queued");
    let back = q
        .queue
        .claim(Lane::Image, "w2")
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(back.attempt, 1);
    q.finish().await;
}
