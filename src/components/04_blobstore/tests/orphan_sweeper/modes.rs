//! AC-1 and AC-2: what each mode does with 100 files with rows, 20 old orphans and 10 young ones.

use sunnycloud_blobstore::OrphanStats;

use crate::rows::START;
use crate::support::{AGE, Scene, Volume};

#[tokio::test]
async fn delete_mode_removes_only_the_old_orphans() {
    // Arrange
    let scene = Scene::new().await;
    let volume = Volume::build(&scene, START).await;
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(
        stats,
        OrphanStats {
            found: 20,
            removed: 20
        }
    );
    assert!(scene.none_exist(&volume.old));
    assert!(scene.all_exist(&volume.young));
    assert!(scene.all_exist(&volume.rowed));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn report_mode_finds_the_old_orphans_and_deletes_nothing() {
    // Arrange
    let scene = Scene::new().await;
    let volume = Volume::build(&scene, START).await;
    let sweeper = scene.sweeper("report", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(
        stats,
        OrphanStats {
            found: 20,
            removed: 0
        }
    );
    assert!(scene.all_exist(&volume.everything()));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn off_mode_does_nothing() {
    // Arrange
    let scene = Scene::new().await;
    let volume = Volume::build(&scene, START).await;
    let sweeper = scene.sweeper("off", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(stats, OrphanStats::default());
    assert!(scene.all_exist(&volume.everything()));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_second_delete_run_finds_nothing_more() {
    // Arrange
    let scene = Scene::new().await;
    let volume = Volume::build(&scene, START).await;
    let sweeper = scene.sweeper("delete", AGE);
    Scene::run(&sweeper, START).await;

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(stats, OrphanStats::default());
    assert!(scene.all_exist(&volume.young));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_file_is_kept_whatever_the_state_of_its_row() {
    // Arrange
    let scene = Scene::new().await;
    let states = ["uploading", "committed", "gc_pending"];
    let mut ids = Vec::new();
    for (n, state) in (0u32..).zip(states) {
        let id = crate::support::blob_id(9, n);
        scene.insert_rows(&[id], state).await;
        scene.file_aged(id, START, 100 * AGE);
        ids.push(id);
    }
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(stats, OrphanStats::default());
    assert!(scene.all_exist(&ids));
    drop(sweeper);
    scene.finish().await;
}
