//! Construction and the `Janitor` port.

use contracts::Janitor;
use sunnycloud_blobstore::OrphanSweeper;

use crate::rows::START;
use crate::support::{AGE, Scene, blob_id};

#[tokio::test]
async fn an_unknown_mode_or_a_missing_blob_root_is_refused() {
    // Arrange
    let scene = Scene::new().await;
    let unknown = scene.config("purge", AGE);
    let mut no_root = scene.config("delete", AGE);
    no_root.blob_root = None;

    // Act
    let refusals = [unknown, no_root].map(|config| {
        OrphanSweeper::new(&config, scene.rig.api.clone(), scene.rig.clock.clone()).is_err()
    });

    // Assert
    assert_eq!(refusals, [true, true]);
    scene.finish().await;
}

#[tokio::test]
async fn the_janitor_reports_the_removed_files_as_processed() {
    // Arrange
    let scene = Scene::new().await;
    for n in 0..3 {
        scene.file_aged(blob_id(2, n), START, 10 * AGE);
    }
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let stats = Janitor::run_cycle(&sweeper, START).await.expect("cycle");

    // Assert
    assert_eq!(sweeper.name(), "orphan_sweeper");
    assert_eq!(stats.processed, 3);
    drop(sweeper);
    scene.finish().await;
}
