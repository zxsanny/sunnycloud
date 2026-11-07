//! AC-3: a scan that is incomplete deletes nothing, even the orphans it found before it failed.

use crate::rows::START;
use crate::support::{AGE, Interference, Plan, Scene, blob_id, make_blocker};

#[tokio::test]
async fn an_unreadable_prefix_directory_aborts_the_run_with_no_deletion() {
    // Arrange
    let scene = Scene::new().await;
    let before = blob_id(1, 1);
    let after = blob_id(3, 1);
    for id in [before, after] {
        scene.file_aged(id, START, 10 * AGE);
    }
    make_blocker(&scene, 2);
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let outcome = sweeper.run_cycle(START).await;

    // Assert
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(scene.all_exist(&[before, after]));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_database_error_mid_run_aborts_the_run_with_no_deletion() {
    // Arrange
    let scene = Scene::new().await;
    let found_first = blob_id(1, 1);
    let found_second = blob_id(3, 1);
    for id in [found_first, found_second] {
        scene.file_aged(id, START, 10 * AGE);
    }
    let hook = Interference::new(&scene, Plan::HideTableAfterListing(3)).await;
    let sweeper = scene.hooked("delete", hook.clone());

    // Act
    let outcome = sweeper.run_cycle(START).await;

    // Assert
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(scene.all_exist(&[found_first, found_second]));
    drop(sweeper);
    hook.close().await;
    scene.finish().await;
}

#[tokio::test]
async fn a_database_error_at_the_re_check_ends_the_delete_phase() {
    // Arrange
    let scene = Scene::new().await;
    let orphans = [blob_id(1, 1), blob_id(3, 1)];
    for id in orphans {
        scene.file_aged(id, START, 10 * AGE);
    }
    let hook = Interference::new(&scene, Plan::HideTableBeforeRecheck).await;
    let sweeper = scene.hooked("delete", hook.clone());

    // Act
    let outcome = sweeper.run_cycle(START).await;

    // Assert
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(scene.all_exist(&orphans));
    drop(sweeper);
    hook.close().await;
    scene.finish().await;
}

#[tokio::test]
async fn report_mode_also_aborts_on_an_unreadable_directory() {
    // Arrange
    let scene = Scene::new().await;
    scene.file_aged(blob_id(1, 1), START, 10 * AGE);
    make_blocker(&scene, 2);
    let sweeper = scene.sweeper("report", AGE);

    // Act
    let outcome = sweeper.run_cycle(START).await;

    // Assert
    assert!(outcome.is_err(), "{outcome:?}");
    drop(sweeper);
    scene.finish().await;
}
