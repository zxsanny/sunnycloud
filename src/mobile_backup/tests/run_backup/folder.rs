//! AC-8: the target folder `Camera Uploads/<device label>/`.

use mobile_backup::Trigger;

use crate::support::server::ROOT;
use crate::support::{LABEL, Rig};

#[tokio::test]
async fn the_folder_is_created_once_its_id_is_kept_in_cursor_and_later_runs_reuse_it() {
    // Arrange
    let rig = Rig::new(8, 50);
    for id in 1..=3 {
        rig.source.add(id, 1024);
    }

    // Act
    rig.run(Trigger::FirstSync).await;
    let (creates, lists) = {
        let record = rig.api.record();
        (record.creates, record.lists)
    };
    rig.source.add(4, 1024);
    rig.source.add(5, BLOCK_PLUS_ONE);
    rig.run(Trigger::MediaChange).await;

    // Assert
    let state = rig.server.state();
    let camera = state
        .folder_named(ROOT, "Camera Uploads")
        .expect("camera folder");
    let device = state.folder_named(camera, LABEL).expect("device folder");
    assert_eq!(creates, 2, "Camera Uploads and the device folder");
    assert_eq!(
        rig.db.cursor_text("camera_folder"),
        Some(device.to_string())
    );
    let record = rig.api.record();
    assert_eq!(
        (record.creates, record.lists),
        (creates, lists),
        "later runs made no folder call"
    );
    assert!(state.files.values().all(|file| file.parent == device));
    assert_eq!(state.files.len(), 5);
}

const BLOCK_PLUS_ONE: usize = 2 * 1024 * 1024 + 1;

#[tokio::test]
async fn an_existing_folder_is_found_and_not_created_again() {
    // Arrange
    let rig = Rig::new(8, 50);
    let camera = rig
        .server
        .create_folder(ROOT, "Camera Uploads")
        .expect("camera")
        .node_id;
    let device = rig
        .server
        .create_folder(camera, LABEL)
        .expect("device")
        .node_id;
    rig.source.add(1, 1024);

    // Act
    rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(rig.api.record().creates, 0);
    assert_eq!(
        rig.db.cursor_text("camera_folder"),
        Some(device.to_string())
    );
    assert!(
        rig.server
            .state()
            .files
            .values()
            .all(|file| file.parent == device)
    );
}

#[tokio::test]
async fn a_run_with_nothing_to_upload_creates_no_folder() {
    // Arrange
    let rig = Rig::new(8, 50);

    // Act
    rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(rig.api.record().creates, 0);
    assert_eq!(rig.db.cursor_text("camera_folder"), None);
}
