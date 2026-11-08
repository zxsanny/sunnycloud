//! AC-1: one `block_fsync_duration_seconds` observation per group flush.

use std::sync::Arc;
use std::time::Duration;

use crate::barrier::Rig;
use crate::{registry, sample};

const COUNT: &str = "sunnycloud_block_fsync_duration_seconds_count";

fn observed_rig(delay: Duration) -> (Arc<Rig>, sc_observability::Metrics) {
    let (metrics, blob) = registry();
    let mut rig = Rig::new(delay);
    rig.barrier = rig.barrier.with_metrics(blob);
    (Arc::new(rig), metrics)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_histogram_count_equals_the_group_flushes_of_a_hundred_writes_over_ten_files() {
    // Arrange
    let (rig, metrics) = observed_rig(Duration::ZERO);
    rig.hook.delay_flushes(Duration::from_millis(5));
    let files: Vec<_> = (0..10).map(|_| rig.scratch.file()).collect();

    // Act
    let waiters: Vec<_> = (0..100)
        .map(|n| {
            let rig = Arc::clone(&rig);
            let (id, file) = files[n % 10].clone();
            tokio::spawn(async move {
                let written = rig.write_and_register(id, &file, (n / 10) as u64 * 4096);
                rig.barrier.await_durable(written.token).await
            })
        })
        .collect();
    for waiter in waiters {
        waiter
            .await
            .expect("task finishes")
            .expect("write is acknowledged");
    }
    let scrape = metrics.render().await.expect("render");

    // Assert
    let flushes = rig.hook.flush_count();
    assert!(flushes <= 25, "{flushes} flushes for 100 writes");
    assert_eq!(sample(&scrape, COUNT), Some(flushes as f64), "{scrape}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_flush_is_observed_too() {
    // Arrange
    let (rig, metrics) = observed_rig(Duration::ZERO);
    rig.hook.fail_next_flushes(1);
    let (id, file) = rig.scratch.file();
    let written = rig.write_and_register(id, &file, 0);

    // Act
    let outcome = rig.barrier.await_durable(written.token).await;
    let scrape = metrics.render().await.expect("render");

    // Assert
    assert!(outcome.is_err());
    assert_eq!(sample(&scrape, COUNT), Some(1.0), "{scrape}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_barrier_without_metrics_flushes_as_before() {
    // Arrange
    let rig = Rig::new(Duration::ZERO);
    let (id, file) = rig.scratch.file();
    let written = rig.write_and_register(id, &file, 0);

    // Act
    let outcome = rig.barrier.await_durable(written.token).await;

    // Assert
    outcome.expect("durable");
    assert_eq!(rig.hook.flush_count(), 1);
}
