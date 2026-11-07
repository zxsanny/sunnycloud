//! Names the sweeper must not treat as orphans: the GC sweeper's parked `<id>.gc` files and anything that is not
//! a blob id. None of them is reported or removed, and none makes the scan partial.

use std::fs;
use std::os::unix::fs::symlink;

use sunnycloud_blobstore::OrphanStats;
use uuid::Uuid;

use crate::rows::START;
use crate::support::{AGE, Scene, blob_id};

fn parked(scene: &Scene, id: Uuid) -> std::path::PathBuf {
    let mut name = scene.path(id).into_os_string();
    name.push(".gc");
    name.into()
}

fn put_parked(scene: &Scene, id: Uuid) -> std::path::PathBuf {
    scene.file_aged(id, START, 10 * AGE);
    let path = parked(scene, id);
    fs::rename(scene.path(id), &path).expect("park the file");
    path
}

#[tokio::test]
async fn a_parked_file_without_a_row_is_neither_reported_nor_deleted() {
    // Arrange
    let scene = Scene::new().await;
    let only_parked = put_parked(&scene, blob_id(4, 1));
    let beside_orphan = put_parked(&scene, blob_id(5, 1));
    let orphan = blob_id(5, 2);
    scene.file_aged(orphan, START, 10 * AGE);
    let delete = scene.sweeper("delete", AGE);
    let report = scene.sweeper("report", AGE);

    // Act
    let reported = Scene::run(&report, START).await;
    let deleted = Scene::run(&delete, START).await;

    // Assert
    assert_eq!(
        reported,
        OrphanStats {
            found: 1,
            removed: 0
        }
    );
    assert_eq!(
        deleted,
        OrphanStats {
            found: 1,
            removed: 1
        }
    );
    assert!(only_parked.exists());
    assert!(beside_orphan.exists());
    assert!(!scene.has_file(orphan));
    drop((delete, report));
    scene.finish().await;
}

#[tokio::test]
async fn a_parked_file_of_a_blob_with_a_row_stays_and_the_scan_completes() {
    // Arrange
    let scene = Scene::new().await;
    let id = blob_id(6, 1);
    scene.insert_rows(&[id], "gc_pending").await;
    let path = put_parked(&scene, id);
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(stats, OrphanStats::default());
    assert!(path.exists());
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn names_that_are_not_blob_ids_are_left_alone_and_do_not_abort_the_scan() {
    // Arrange
    let scene = Scene::new().await;
    let orphan = blob_id(8, 1);
    scene.file_aged(orphan, START, 10 * AGE);
    let folder = scene.path(orphan).parent().expect("folder").to_path_buf();
    let stray = folder.join("notes.txt");
    fs::write(&stray, b"x").expect("stray file");
    let uppercase = folder.join(blob_id(8, 2).to_string().to_uppercase());
    fs::write(&uppercase, b"x").expect("uppercase name");
    let subfolder = folder.join(blob_id(8, 3).to_string());
    fs::create_dir(&subfolder).expect("folder named like a blob");
    let link = folder.join(blob_id(8, 4).to_string());
    symlink(&stray, &link).expect("link named like a blob");
    let misplaced = scene
        .rig
        .root
        .join("blobs/09/09")
        .join(blob_id(8, 5).to_string());
    fs::create_dir_all(misplaced.parent().expect("folder")).expect("other folder");
    fs::write(&misplaced, b"x").expect("misplaced file");
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(
        stats,
        OrphanStats {
            found: 1,
            removed: 1
        }
    );
    assert!(!scene.has_file(orphan));
    for kept in [&stray, &uppercase, &subfolder, &link, &misplaced] {
        assert!(
            kept.symlink_metadata().is_ok(),
            "{} was removed",
            kept.display()
        );
    }
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_volume_without_a_blob_folder_has_no_orphans() {
    // Arrange
    let scene = Scene::new().await;
    let sweeper = scene.sweeper("delete", AGE);

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(stats, OrphanStats::default());
    drop(sweeper);
    scene.finish().await;
}
