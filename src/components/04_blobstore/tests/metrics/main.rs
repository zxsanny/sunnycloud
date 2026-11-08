//! Blob I/O metrics (AZ-3748): the four series in a real registry, fed by the real barrier, reader and health
//! probe. Flush counts come from the `sync-hook` feature's own counter.

#[allow(dead_code)]
#[path = "../barrier/support.rs"]
mod barrier;
#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

mod flush;
mod read;
mod volume;

use std::sync::Arc;

use sc_clock::SystemClock;
use sc_observability::{BuildInfo, Metrics};
use sunnycloud_blobstore::BlobMetrics;

pub fn registry() -> (Metrics, Arc<BlobMetrics>) {
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: false,
    })
    .expect("metrics");
    let blob = BlobMetrics::register(&metrics, Arc::new(SystemClock::new())).expect("blob series");
    (metrics, blob)
}

/// The value of the sample whose series (name and labels, as scraped) is `series`.
pub fn sample(scrape: &str, series: &str) -> Option<f64> {
    scrape
        .lines()
        .find_map(|line| line.strip_prefix(series)?.strip_prefix(' ')?.parse().ok())
}
