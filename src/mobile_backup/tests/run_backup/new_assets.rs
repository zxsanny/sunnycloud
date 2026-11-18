//! AC-2: new assets from the change token, rows stored before the token moves.

use std::sync::atomic::Ordering;

use mobile_backup::{AssetSource, RunError, Trigger};

use crate::support::Rig;
use crate::support::source::token_of;

#[tokio::test]
async fn rows_of_a_paged_listing_exist_before_the_stored_token_advances_to_the_current_token() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.cap_pages(100);
    rig.run(Trigger::MediaChange).await;
    let stored_before = rig
        .db
        .cursor_bytes("change_token")
        .expect("the first run stored a token");
    for id in 1..=250 {
        rig.source.add(id, 1024);
    }
    let target = rig.source.current_token();
    let pages_before = rig.source.listed_pages.load(Ordering::SeqCst);
    rig.db.fail_token_writes();

    // Act
    let failed = rig.try_run(Trigger::MediaChange).await;

    // Assert
    assert!(matches!(failed, Err(RunError::Checkpoint(_))), "{failed:?}");
    assert_eq!(
        rig.db.count("new"),
        250,
        "rows were stored before the token write"
    );
    assert_eq!(rig.db.cursor_bytes("change_token"), Some(stored_before));
    assert!(
        rig.source.listed_pages.load(Ordering::SeqCst) - pages_before >= 3,
        "a paged listing"
    );

    // Act
    rig.db.heal_token_writes();
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert_eq!(rig.db.cursor_bytes("change_token"), Some(target.0));
    assert_eq!(rig.db.count("uploaded"), 250);
}

#[tokio::test]
async fn the_change_token_is_stored_and_returned_as_the_opaque_bytes_the_source_gave() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 1024);
    let issued = token_of(1).0;
    assert!(
        std::str::from_utf8(&issued).is_err(),
        "the fake token is not text"
    );

    // Act
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert_eq!(rig.db.cursor_bytes("change_token"), Some(issued));
}

#[tokio::test]
async fn a_run_with_nothing_new_writes_nothing_to_the_asset_table_and_keeps_the_token() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 1024);
    rig.run(Trigger::MediaChange).await;
    let stored = rig.db.cursor_bytes("change_token");
    rig.db.fail_asset_writes();

    // Act
    let outcome = rig.try_run(Trigger::MediaChange).await;

    // Assert
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(rig.db.cursor_bytes("change_token"), stored);
}
