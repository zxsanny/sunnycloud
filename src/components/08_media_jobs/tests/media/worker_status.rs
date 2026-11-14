//! AZ-3843 AC-3 (IT-08-20 status part): `record_status` keeps one `worker_status` row per worker id, replaced
//! by each report, with `seen_at` on the API clock whatever the supervisor's own clock says.

use std::sync::Arc;
use std::time::Duration;

use contracts::{WorkerJobs, WorkerStatus};
use media_api::WorkerJobsFacade;
use sc_clock::Clock;
use sqlx::Connection;

use crate::accept_support::Accept;

const HOUR: Duration = Duration::from_secs(3600);

type StatusRow = (String, i64, i32, bool);

async fn rows(a: &Accept) -> Vec<StatusRow> {
    let mut conn = a.q.fixture.owner_conn().await;
    let rows = sqlx::query_as(
        "SELECT worker_id, (extract(epoch FROM seen_at) * 1000000)::bigint, image_age_days, decoder_ok \
         FROM worker_status ORDER BY worker_id",
    )
    .fetch_all(&mut conn)
    .await
    .expect("worker_status rows");
    conn.close().await.expect("close owner connection");
    rows
}

#[tokio::test]
async fn two_reports_leave_one_row_updated_by_the_second_with_seen_at_on_the_api_clock() {
    // Arrange
    let a = Accept::new().await;
    let jobs = WorkerJobsFacade::new(
        a.q.fixture.api.clone(),
        a.q.clock.clone(),
        a.q.queue.clone(),
        Arc::clone(&a.store),
    );
    // The supervisor's clock runs an hour ahead; nothing of it reaches the report.
    let supervisor_now = a.q.clock.now_utc().as_unix_micros() + HOUR.as_micros() as i64;
    jobs.record_status(
        "gpu-1",
        WorkerStatus {
            decoder_ok: false,
            image_age_days: 9,
        },
    )
    .await
    .expect("first report");
    a.q.clock.tick(Duration::from_secs(30));

    // Act
    jobs.record_status(
        "gpu-1",
        WorkerStatus {
            decoder_ok: true,
            image_age_days: 3,
        },
    )
    .await
    .expect("second report");

    // Assert
    let api_now = a.q.clock.now_utc().as_unix_micros();
    let found = rows(&a).await;
    assert_eq!(found, vec![("gpu-1".to_owned(), api_now, 3, true)]);
    assert_ne!(found[0].1, supervisor_now);
    drop(jobs);
    a.finish().await;
}

#[tokio::test]
async fn an_age_beyond_the_column_is_an_unexpected_failure_and_writes_nothing() {
    // Arrange
    let a = Accept::new().await;
    let jobs = WorkerJobsFacade::new(
        a.q.fixture.api.clone(),
        a.q.clock.clone(),
        a.q.queue.clone(),
        Arc::clone(&a.store),
    );

    // Act
    let result = jobs
        .record_status(
            "gpu-1",
            WorkerStatus {
                decoder_ok: true,
                image_age_days: u32::MAX,
            },
        )
        .await;

    // Assert
    assert!(matches!(
        result,
        Err(contracts::WorkerJobError::Unexpected(_))
    ));
    assert!(rows(&a).await.is_empty());
    drop(jobs);
    a.finish().await;
}
