//! AC-3: the health probe and the blobstore `HealthContributor`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use contracts::{BlobError, HealthContributor, HealthStatus};
use rustix::fs::statvfs;
use sunnycloud_blobstore::{BlobHealth, BlobHealthContributor};

use crate::support::Rig;

const MIB: u64 = 1 << 20;

fn health(rig: &Rig, reserve: u64) -> Arc<BlobHealth> {
    let mut config = rig.config.clone();
    config.blob_free_reserve = reserve;
    Arc::new(BlobHealth::new(&config, rig.api.clone()).expect("health"))
}

fn filesystem_free(rig: &Rig) -> u64 {
    let stat = statvfs(&rig.volume).expect("statvfs");
    stat.f_bavail * stat.f_frsize
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_count_is_the_number_of_rows_with_the_mark_and_the_contributor_is_degraded() {
    // Arrange
    let rig = Rig::new().await;
    let mut ids = Vec::new();
    for seed in 0..8 {
        ids.push(rig.store(1024, seed).await.blob_id);
    }
    let probe = health(&rig, 0);
    let contributor = BlobHealthContributor::new(Arc::clone(&probe));
    let clean = (
        probe.probe().await.expect("probe"),
        contributor.check().await,
    );
    for id in &ids[..5] {
        rig.db
            .admin_exec(&format!(
                "UPDATE blobs SET unavailable_at = 'epoch'::timestamptz WHERE id = '{id}'"
            ))
            .await;
    }

    // Act
    let marked = probe.probe().await.expect("probe");
    let item = contributor.check().await;

    // Assert
    assert_eq!(clean.0.unavailable_count, 0);
    assert_eq!(clean.1.status, HealthStatus::Ok);
    assert_eq!(marked.unavailable_count, 5);
    assert_eq!(item.status, HealthStatus::Degraded);
    assert!(!item.detail.is_empty());
    assert_eq!(contributor.name(), "blobstore");
    drop((probe, contributor));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_root_is_not_writable_and_a_writable_one_is() {
    // Arrange
    let rig = Rig::new().await;
    let probe = health(&rig, 0);
    let writable = probe.probe().await.expect("probe");
    fs::set_permissions(&rig.volume, fs::Permissions::from_mode(0o555)).expect("read-only root");

    // Act
    let read_only = probe.probe().await;
    fs::set_permissions(&rig.volume, fs::Permissions::from_mode(0o755)).expect("restore");

    // Assert
    assert!(writable.writable);
    assert!(!read_only.expect("probe of a read-only root").writable);
    drop(probe);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn free_bytes_follow_the_filesystem_and_the_floor_decides_below_floor() {
    // Arrange
    let rig = Rig::new().await;
    let free = filesystem_free(&rig);
    let under_floor = health(&rig, free + 64 * MIB);
    let above_floor = health(&rig, 1);
    let no_floor = health(&rig, 0);

    // Act
    let under = under_floor.probe().await.expect("probe");
    let above = above_floor.probe().await.expect("probe");
    let none = no_floor.probe().await.expect("probe");
    let after = filesystem_free(&rig);

    // Assert
    assert!(under.below_floor);
    assert!(!above.below_floor);
    assert!(!none.below_floor);
    for state in [under, above, none] {
        assert!(
            state.free_bytes.abs_diff(free) <= MIB && state.free_bytes.abs_diff(after) <= MIB,
            "{} against {free} and {after}",
            state.free_bytes
        );
    }
    drop((under_floor, above_floor, no_floor));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_that_is_not_there_fails_the_probe_and_takes_the_contributor_down() {
    // Arrange
    let rig = Rig::new().await;
    let mut config = rig.config.clone();
    config.blob_root = Some(
        rig.volume
            .join("not-mounted")
            .to_string_lossy()
            .into_owned(),
    );
    let probe = Arc::new(BlobHealth::new(&config, rig.api.clone()).expect("health"));
    let contributor = BlobHealthContributor::new(Arc::clone(&probe));

    // Act
    let result = probe.probe().await;
    let item = contributor.check().await;

    // Assert
    assert!(matches!(result, Err(BlobError::Io(_))), "{result:?}");
    assert_eq!(item.status, HealthStatus::Down);
    drop((probe, contributor));
    rig.finish().await;
}
