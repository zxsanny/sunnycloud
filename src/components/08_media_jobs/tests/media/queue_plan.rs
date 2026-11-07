//! PT-08-1 (AC-4 of the tables task): the claim statement the queue really runs keeps the claim index with
//! 100,000 queued jobs, and the queue claims from that table in priority order.

use std::sync::Arc;

use media_api::{JobMetrics, JobQueue, explain_claim};
use media_shared::types::Lane;
use sc_clock::ManualClock;
use sc_observability::{BuildInfo, Metrics};
use sqlx::Connection;

use crate::plans::{assert_claim_plan, seed_queue};
use crate::queue_support::{ATTEMPTS, config};
use crate::support::START;

// The seeded due times run from 2026-11-05 12:00:00Z for a day; this reading is 2026-11-06 12:00:00Z.
const AFTER_SEEDED_DAY: i64 = START.as_unix_micros() + 86_400_000_000;

#[tokio::test]
async fn the_real_claim_statement_walks_the_claim_index_on_both_lanes_with_100000_queued_jobs() {
    // Arrange
    let queue = seed_queue().await;
    let now = sc_clock::Timestamp::from_unix_micros(AFTER_SEEDED_DAY);

    // Act
    let image = explain_claim(&queue.fixture.api, Lane::Image, now, ATTEMPTS as i64)
        .await
        .expect("image plan");
    let video = explain_claim(&queue.fixture.api, Lane::Video, now, ATTEMPTS as i64)
        .await
        .expect("video plan");

    // Assert
    assert_claim_plan(&image, "image");
    assert_claim_plan(&video, "video");
    queue.fixture.finish().await;
}

#[tokio::test]
async fn claims_on_the_deep_queue_return_the_interactive_job_with_the_earliest_due_time_first() {
    // Arrange
    let seeded = seed_queue().await;
    let clock = Arc::new(ManualClock::new(sc_clock::Timestamp::from_unix_micros(
        AFTER_SEEDED_DAY,
    )));
    let metrics = Arc::new(
        JobMetrics::register(
            &Metrics::new(BuildInfo {
                version: "test",
                commit: "test",
                test_clock: true,
            })
            .expect("metrics"),
        )
        .expect("job metrics"),
    );
    let queue = JobQueue::new(seeded.fixture.api.clone(), clock, &config(), metrics);

    // Act
    let first = queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    let second = queue
        .claim(Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");

    // Assert
    let mut conn = seeded.fixture.owner_conn().await;
    let ids = [first.job_id, second.job_id];
    for id in ids {
        let (priority, status, attempts): (i16, String, i16) =
            sqlx::query_as("SELECT priority, status, attempts FROM jobs WHERE id = $1")
                .bind(id)
                .fetch_one(&mut conn)
                .await
                .expect("row");
        assert_eq!((priority, status.as_str(), attempts), (0, "running", 1));
    }
    let lane_leased: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE status = 'running' AND lane = 'video'")
            .fetch_one(&mut conn)
            .await
            .expect("count");
    assert_eq!(lane_leased, 0);
    assert_ne!(ids[0], ids[1]);
    conn.close().await.expect("close owner connection");
    drop(queue);
    seeded.fixture.finish().await;
}
