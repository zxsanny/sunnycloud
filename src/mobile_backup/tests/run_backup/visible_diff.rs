//! AC-3: the visible-id diff finds older photos a change token cannot reveal.

use std::sync::atomic::Ordering;

use mobile_backup::Trigger;

use crate::support::Rig;

/// Limited access, 20 visible photos uploaded, 10 older ones (token below the cursor) not yet granted.
async fn limited_rig_after_the_first_run() -> Rig {
    let rig = Rig::new(8, 50);
    rig.source.set_limited(true);
    for id in 21..=30 {
        rig.source.add_hidden(id, 1024);
    }
    for id in 1..=20 {
        rig.source.add(id, 1024);
    }
    rig.run(Trigger::FirstSync).await;
    assert_eq!(rig.server.file_count(), 20);
    rig
}

fn id_pages(rig: &Rig) -> usize {
    rig.source.visible_id_pages.load(Ordering::SeqCst)
}

#[tokio::test]
async fn picked_older_photos_are_queued_when_the_visible_count_changed() {
    // Arrange
    let rig = limited_rig_after_the_first_run().await;
    for id in 21..=30 {
        rig.source.grant_keeping_token(id);
    }
    let (acked_before, pages_before) = (
        rig.api.record().acked,
        rig.source.listed_pages.load(Ordering::SeqCst),
    );

    // Act
    rig.run(Trigger::MediaChange).await;

    // Assert
    let mut described = rig.source.described.lock().expect("described").clone();
    described.sort_unstable();
    assert_eq!(
        described,
        (21..=30).collect::<Vec<i64>>(),
        "exactly the 10 older assets were queued"
    );
    assert_eq!(rig.api.record().acked - acked_before, 10);
    assert_eq!(rig.server.file_count(), 30);
    assert_eq!(rig.db.count("uploaded"), 30);
    assert_eq!(
        rig.source.listed_pages.load(Ordering::SeqCst) - pages_before,
        1,
        "the change token listed nothing: only the diff found them"
    );
}

#[tokio::test]
async fn a_catch_up_run_does_the_diff_and_queues_nothing_when_nothing_changed() {
    // Arrange
    let rig = limited_rig_after_the_first_run().await;
    let described_before = rig.source.described.lock().expect("described").len();
    let pages_before = id_pages(&rig);
    let acked_before = rig.api.record().acked;

    // Act
    rig.run(Trigger::CatchUp).await;

    // Assert
    assert!(
        id_pages(&rig) > pages_before,
        "the diff listed the visible ids"
    );
    assert_eq!(
        rig.source.described.lock().expect("described").len(),
        described_before
    );
    assert_eq!(rig.api.record().acked, acked_before);
}

#[tokio::test]
async fn a_run_with_unchanged_access_level_and_count_does_no_diff() {
    // Arrange
    let rig = limited_rig_after_the_first_run().await;
    let pages_before = id_pages(&rig);

    // Act
    rig.run(Trigger::MediaChange).await;
    rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(id_pages(&rig), pages_before);
}

#[tokio::test]
async fn a_changed_access_level_triggers_the_diff_even_when_the_count_is_the_same() {
    // Arrange
    let rig = limited_rig_after_the_first_run().await;
    rig.source.set_limited(false);
    let pages_before = id_pages(&rig);

    // Act
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert!(id_pages(&rig) > pages_before);
}
