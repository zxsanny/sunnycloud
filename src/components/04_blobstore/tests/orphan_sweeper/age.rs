//! AC-68: the orphan of an interrupted `write_complete` is removed only once it is older than the age, and the
//! file of an `uploading` blob is never removed.

use std::time::Duration;

use sunnycloud_blobstore::OrphanStats;

use crate::rows::{DAY, START};
use crate::support::{Scene, blob_id};

const MILLI: Duration = Duration::from_millis(1);

#[tokio::test]
async fn only_the_rowless_file_is_removed_and_only_after_the_age() {
    // Arrange
    let scene = Scene::new().await;
    let age = 2 * DAY;
    let rowless = blob_id(7, 1);
    let uploading = blob_id(7, 2);
    scene.file_aged(rowless, START, Duration::ZERO);
    scene.file_aged(uploading, START, Duration::ZERO);
    scene.insert_rows(&[uploading], "uploading").await;
    let sweeper = scene.sweeper("delete", age);
    let at = |by: Duration| START.checked_add(by).expect("later");

    // Act
    let before = Scene::run(&sweeper, at(age - MILLI)).await;
    let files_before = (scene.has_file(rowless), scene.has_file(uploading));
    let exactly = Scene::run(&sweeper, at(age)).await;
    let past = Scene::run(&sweeper, at(age + MILLI)).await;

    // Assert
    assert_eq!(before, OrphanStats::default());
    assert_eq!(files_before, (true, true));
    assert_eq!(exactly, OrphanStats::default());
    assert_eq!(
        past,
        OrphanStats {
            found: 1,
            removed: 1
        }
    );
    assert!(!scene.has_file(rowless));
    assert!(scene.has_file(uploading));
    drop(sweeper);
    scene.finish().await;
}
