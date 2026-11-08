//! AC-2: the probe sets the two free-space gauges.

use std::sync::Arc;

use sunnycloud_blobstore::BlobHealth;

use crate::support::Rig;
use crate::{registry, sample};

const MIB: u64 = 1 << 20;

#[tokio::test(flavor = "multi_thread")]
async fn the_probe_sets_free_bytes_to_its_figure_and_the_floor_to_the_reserve() {
    // Arrange
    let rig = Rig::new().await;
    let (metrics, blob) = registry();
    let mut config = rig.config.clone();
    config.blob_free_reserve = 8 * MIB;
    let health = Arc::new(
        BlobHealth::new(&config, rig.api.clone())
            .expect("health")
            .with_metrics(blob),
    );
    let before = sample(
        &metrics.render().await.expect("render"),
        "sunnycloud_blob_free_bytes",
    );

    // Act
    let state = health.probe().await.expect("probe");
    let scrape = metrics.render().await.expect("render");

    // Assert
    assert_eq!(before, Some(0.0));
    assert_eq!(
        sample(&scrape, "sunnycloud_blob_free_bytes"),
        Some(state.free_bytes as f64)
    );
    assert_eq!(
        sample(&scrape, "sunnycloud_blob_free_floor_bytes"),
        Some((8 * MIB) as f64)
    );
    drop(health);
    rig.finish().await;
}
