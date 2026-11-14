//! AZ-3845 AC-2 (IT-08-15 derived janitor) and AC-3 (IT-08-15 reaper, NFT-RES-23): the two maintenance cycles on
//! the real queue and the real 04 `GcQueue`.

use std::time::Duration;

use contracts::Janitor;
use media_api::{DerivedJanitor, JobReaper, Priority};
use media_shared::types::{Disposition, JobKind, Lane, SkipOutcome};
use sqlx::Connection;
use uuid::Uuid;

use crate::accept_support::Accept;
use crate::maint_support::{derived_blobs, gc_rows, seed_derived, worker_seen};
use crate::queue_support::{LEASE, Queue, config, micros};

const SECOND: Duration = Duration::from_secs(1);

#[tokio::test]
async fn the_derived_janitor_removes_only_rows_whose_content_has_no_revision_left() {
    // Arrange
    let a = Accept::new().await;
    let (other, _) = a.q.second_account().await;
    let mut conn = a.q.fixture.owner_conn().await;
    let mut orphans = seed_derived(&mut conn, a.q.account, None, 150).await;
    // Another account's revision content does not keep this account's row.
    orphans.extend(seed_derived(&mut conn, a.q.account, Some(&[2; 32]), 1).await);
    orphans.extend(seed_derived(&mut conn, other, None, 2).await);
    let mut live = seed_derived(&mut conn, a.q.account, Some(&[1; 32]), 5).await;
    live.extend(seed_derived(&mut conn, other, Some(&[2; 32]), 3).await);
    conn.close().await.expect("close owner connection");
    orphans.sort();
    live.sort();
    let janitor = DerivedJanitor::new(a.q.fixture.api.clone(), a.q.clock.clone(), a.gc.clone());

    // Act
    let stats = janitor.run_cycle(a.q.clock_now()).await.expect("cycle");
    let again = janitor
        .run_cycle(a.q.clock_now())
        .await
        .expect("second cycle");

    // Assert
    assert_eq!((stats.processed, again.processed), (153, 0));
    assert_eq!(derived_blobs(&a.q).await, live);
    let mut queued = gc_rows(&a.q, a.q.account).await;
    queued.extend(gc_rows(&a.q, other).await);
    queued.sort();
    let expected: Vec<(Uuid, String, bool)> = orphans
        .iter()
        .map(|blob| (*blob, "derived_removed".to_owned(), true))
        .collect();
    assert_eq!(queued, expected);
    drop(janitor);
    a.finish().await;
}

/// Adds a stills job, leases it and closes it as skipped or dead.
async fn finished(q: &Queue, n: u32, dead: bool) -> Uuid {
    let id = q.add(JobKind::Stills, n, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
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

async fn worker_ids(q: &Queue) -> Vec<String> {
    let mut conn = q.fixture.owner_conn().await;
    let ids = sqlx::query_scalar("SELECT worker_id FROM worker_status ORDER BY worker_id")
        .fetch_all(&mut conn)
        .await
        .expect("worker ids");
    conn.close().await.expect("close owner connection");
    ids
}

#[tokio::test]
async fn the_job_reaper_marks_expired_exhausted_jobs_dead_and_purges_old_terminal_and_worker_rows()
{
    // Arrange
    let q = Queue::new().await;
    let ttl = config().job_terminal_ttl;
    let old_skipped = finished(&q, 1, false).await;
    let old_dead = finished(&q, 2, true).await;
    worker_seen(&q, "gone-worker", micros(&q.clock)).await;
    q.clock.tick(ttl + SECOND);
    let recent = finished(&q, 3, false).await;
    let exhausted = q.add(JobKind::Poster, 4, Priority::Interactive).await;
    for _ in 0..3 {
        q.queue
            .claim(Lane::Video, "w")
            .await
            .expect("claim")
            .expect("job");
        q.clock.tick(LEASE + SECOND);
    }
    worker_seen(&q, "live-worker", micros(&q.clock)).await;
    let reaper = JobReaper::new(
        q.fixture.api.clone(),
        q.clock.clone(),
        q.queue.clone(),
        &config(),
    );

    // Act
    let stats = reaper.run_cycle(q.clock_now()).await.expect("cycle");

    // Assert
    assert_eq!(stats.processed, 4);
    let mut conn = q.fixture.owner_conn().await;
    let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM jobs ORDER BY id")
        .fetch_all(&mut conn)
        .await
        .expect("job ids");
    conn.close().await.expect("close owner connection");
    for gone in [old_skipped, old_dead] {
        assert!(!left.contains(&gone));
    }
    assert!(left.contains(&recent));
    assert_eq!(q.row(exhausted).await.status, "dead");
    assert_eq!(worker_ids(&q).await, ["live-worker"]);
    drop(reaper);
    q.finish().await;
}
