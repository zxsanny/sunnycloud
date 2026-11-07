//! AC-4: a writer that inserts its row after the listing and before the delete keeps its file.

use sunnycloud_blobstore::OrphanStats;

use crate::rows::START;
use crate::support::{AGE, Interference, Plan, Scene, blob_id};

#[tokio::test]
async fn a_row_inserted_after_the_listing_keeps_the_file() {
    // Arrange
    let scene = Scene::new().await;
    let racing = blob_id(1, 1);
    let plain = blob_id(1, 2);
    for id in [racing, plain] {
        scene.file_aged(id, START, 10 * AGE);
    }
    let hook = Interference::new(&scene, Plan::RowBeforeRecheck(racing, scene.account)).await;
    let sweeper = scene.hooked("delete", hook.clone());

    // Act
    let stats = Scene::run(&sweeper, START).await;

    // Assert
    assert_eq!(
        stats,
        OrphanStats {
            found: 2,
            removed: 1
        }
    );
    assert!(scene.has_file(racing));
    assert!(!scene.has_file(plain));
    drop(sweeper);
    hook.close().await;
    scene.finish().await;
}
