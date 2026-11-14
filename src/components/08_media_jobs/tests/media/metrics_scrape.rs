//! AZ-3846 AC-4: the six media metrics in a scrape of the registry, after one job that succeeds with outputs and
//! one that dies, with the queue gauges set from the table by the `MediaAdminService` probe.

use std::sync::Arc;

use contracts::DerivedKind;
use media_api::{MediaAdminService, Priority};
use media_shared::types::{Disposition, JobKind, Lane};
use sc_observability::NoLabels;

use crate::accept_support::{Accept, WORKER};
use crate::blob_support::WEBP;

/// The value of the sample `series` (name and labels exactly as rendered), if the scrape has it.
fn sample(scrape: &str, series: &str) -> Option<f64> {
    scrape.lines().find_map(|line| {
        let value = line.strip_prefix(series)?.strip_prefix(' ')?;
        value.parse().ok()
    })
}

#[tokio::test]
async fn a_scrape_has_the_six_metrics_with_kind_labels_and_gauges_equal_to_the_table() {
    // Arrange
    let a = Accept::new().await;
    a.q.registry.register_probe(Arc::new(MediaAdminService::new(
        a.q.fixture.api.clone(),
        a.q.queue.clone(),
        a.q.metrics.clone(),
    )));
    let dead_before = a.q.metrics.dead.get(NoLabels);
    let stills = a.claimed(JobKind::Stills, 1).await;
    a.post_webp(stills, DerivedKind::Thumb).await;
    a.post_webp(stills, DerivedKind::Preview).await;
    let poster = a.claimed(JobKind::Poster, 2).await;
    a.q.queue
        .fail(poster, WORKER, "corrupt", Disposition::Permanent)
        .await
        .expect("fail");
    a.q.add(JobKind::Transcode, 3, Priority::Interactive).await;
    a.q.add(JobKind::Stills, 4, Priority::Interactive).await;
    a.q.queue
        .claim(Lane::Image, WORKER)
        .await
        .expect("claim")
        .expect("job");

    // Act
    let scrape = a.q.registry.render().await.expect("scrape");

    // Assert
    assert_eq!(a.q.row(stills).await.status, "succeeded");
    assert_eq!(a.q.metrics.dead.get(NoLabels) - dead_before, 1);
    let webp = WEBP.len() as f64;
    for (series, value) in [
        ("sunnycloud_jobs_queued", 1.0),
        ("sunnycloud_jobs_running", 1.0),
        ("sunnycloud_job_leases_expired_total", 0.0),
        ("sunnycloud_jobs_dead_total", 1.0),
        (
            "sunnycloud_job_duration_seconds_count{kind=\"stills\"}",
            1.0,
        ),
        (
            "sunnycloud_job_duration_seconds_count{kind=\"poster\"}",
            1.0,
        ),
        (
            "sunnycloud_derived_bytes_total{kind=\"stills\"}",
            2.0 * webp,
        ),
        ("sunnycloud_derived_bytes_total{kind=\"poster\"}", 0.0),
    ] {
        assert_eq!(
            sample(&scrape, series),
            Some(value),
            "{series} in\n{scrape}"
        );
    }
    a.finish().await;
}
