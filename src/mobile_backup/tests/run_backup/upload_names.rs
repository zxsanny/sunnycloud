//! AZ-3997 AC-1: two assets with one display name never meet in the camera folder; the name is chosen once.

use mobile_backup::{BackupOutcome, Trigger};

use crate::support::Rig;

const LARGE: usize = 2 * 1024 * 1024 + 1;
const SMALL: usize = 1024;

fn upload_name(rig: &Rig, asset_id: i64) -> String {
    rig.db
        .asset(asset_id)
        .and_then(|row| row.upload_name)
        .expect("a stored upload name")
}

fn server_names(rig: &Rig) -> Vec<String> {
    let mut names: Vec<String> = rig
        .server
        .state()
        .files
        .values()
        .map(|file| file.name.clone())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn two_assets_named_alike_upload_under_distinct_names_and_the_first_keeps_its_name() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add_named(101, SMALL, "IMG_0001.jpg");
    rig.source.add_named(202, LARGE, "IMG_0001.jpg");
    rig.source.add_named(303, SMALL, "IMG_0002.jpg");

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(
        server_names(&rig),
        ["IMG_0001 (202).jpg", "IMG_0001.jpg", "IMG_0002.jpg"],
        "no server-named conflict copy, both upload paths"
    );
    assert_eq!(
        [101, 202, 303].map(|id| upload_name(&rig, id)),
        ["IMG_0001.jpg", "IMG_0001 (202).jpg", "IMG_0002.jpg"]
    );
}

#[tokio::test]
async fn a_name_that_differs_only_in_case_counts_as_taken() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add_named(1, SMALL, "IMG_0001.JPG");
    rig.source.add_named(2, SMALL, "img_0001.jpg");

    // Act
    rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(
        [1, 2].map(|id| upload_name(&rig, id)),
        ["IMG_0001.JPG", "img_0001 (2).jpg"]
    );
}

#[tokio::test]
async fn names_are_chosen_once_a_later_run_and_an_edit_keep_them() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add_named(101, SMALL, "IMG_0001.jpg");
    rig.source.add_named(202, SMALL, "IMG_0001.jpg");
    rig.run(Trigger::FirstSync).await;
    rig.source.add_named(303, SMALL, "IMG_0001.jpg");

    // Act
    rig.run(Trigger::MediaChange).await;
    rig.source.edit(101);
    rig.source.edit(202);
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert_eq!(
        [101, 202, 303].map(|id| upload_name(&rig, id)),
        ["IMG_0001.jpg", "IMG_0001 (202).jpg", "IMG_0001 (303).jpg"]
    );
}

#[tokio::test]
async fn the_name_check_is_answered_from_the_name_index() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add_named(1, SMALL, "IMG_0001.jpg");
    rig.run(Trigger::FirstSync).await;
    let connection = rusqlite::Connection::open(&rig.path).expect("open the checkpoint");

    // Act
    let plan: String = connection
        .query_row(
            "EXPLAIN QUERY PLAN SELECT 1 FROM asset WHERE lower(upload_name) = lower(?1) AND asset_id != ?2 LIMIT 1",
            rusqlite::params!["IMG_0001.jpg", 2],
            |row| row.get(3),
        )
        .expect("query plan");

    // Assert
    assert!(plan.contains("asset_upload_name_lower"), "{plan}");
}
