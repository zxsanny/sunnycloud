//! AC-7: routing by size and the client metadata of every commit.

use mobile_backup::{BackupOutcome, Trigger};
use serde_json::json;
use sync_core::UploadState;

use crate::support::{DEVICE, Rig};

const BLOCK: usize = 2 * 1024 * 1024;

#[tokio::test]
async fn sizes_route_to_the_staged_or_block_path_small_files_batch_by_the_server_limit_and_commits_carry_client_meta()
 {
    // Arrange: ids 1..=3 are 0 bytes, 2 MiB and 2 MiB + 1; 120 small assets follow.
    let rig = Rig::new(200, 50);
    rig.source.add(1, 0);
    rig.source.add(2, BLOCK);
    rig.source.add(3, BLOCK + 1);
    for id in 4..=123 {
        rig.source.add(id, 1024);
    }

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    let record = rig.api.record();
    assert_eq!(
        record.groups,
        vec![2, 50, 50, 20],
        "0 B and 2 MiB staged, then 120 small assets in batches of 50"
    );
    assert_eq!(record.large.len(), 1);
    let large = &record.large[0];
    assert_eq!(large.local_key, "3", "only 2 MiB + 1 takes the block path");
    assert_eq!(
        large.name, "IMG_3.jpg",
        "the upload name is the display name"
    );
    let session = large
        .session
        .as_ref()
        .expect("a session row before the upload");
    assert_eq!(session.state, UploadState::Uploading);
    assert_eq!(session.block_hashes.len(), 2);
    assert_eq!(
        rig.db.session_count(),
        0,
        "the session row ends with the upload"
    );
    assert_eq!(rig.server.file_count(), 123);
    assert_eq!(rig.db.count("uploaded"), 123);

    let folder = rig.db.cursor_text("camera_folder").expect("folder id");
    assert_eq!(large.parent.to_string(), folder);
    assert_eq!(
        large.meta,
        Some(json!({ "device": DEVICE.to_string(), "asset_id": "3" }))
    );
    let keys = rig.db.keys();
    let mut checked = 0;
    for commit in &record.commits {
        for (key, parent, meta) in &commit.entries {
            let asset = keys
                .iter()
                .find(|(_, found)| *found == key)
                .map(|(id, _)| *id)
                .expect("a stored key");
            assert_eq!(
                meta,
                &Some(json!({ "device": DEVICE.to_string(), "asset_id": asset.to_string() }))
            );
            assert_eq!(parent.to_string(), folder);
            checked += 1;
        }
    }
    assert_eq!(checked, 122, "every staged entry carried client_meta");
}
