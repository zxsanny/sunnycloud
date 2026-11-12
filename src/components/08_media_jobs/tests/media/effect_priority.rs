//! IT-08-3 (AZ-3841 AC-3): bulk priority from the import hint and from the account's queued jobs (threshold 5).
//! The limit of the counting probe (threshold + 1) is pinned by the planner's unit test.

use contracts::PublishedEntry;
use media_api::Priority;
use media_shared::types::JobKind;

use crate::effect_support::{
    Content, JPEG, Job, MP4, current, effect, entry, entry_of, jobs, jobs_of, run,
};
use crate::queue_support::Queue;

#[tokio::test]
async fn entries_with_the_import_hint_are_bulk() {
    // Arrange
    let q = Queue::new().await;
    let effect = current(&q);
    let photo = PublishedEntry {
        import_hint: true,
        ..entry(&q, &Content::new(1, JPEG)).await
    };
    let video = PublishedEntry {
        import_hint: true,
        ..entry(&q, &Content::new(2, MP4)).await
    };

    // Act
    for published in [&photo, &video] {
        run(&q, &effect, q.account, published)
            .await
            .expect("the effect runs");
    }

    // Assert
    assert_eq!(
        jobs(&q, &photo.content_hash).await,
        [Job::new("stills", "image", 1)]
    );
    assert_eq!(
        jobs(&q, &video.content_hash).await,
        [
            Job::new("poster", "video", 1),
            Job::new("transcode", "video", 1)
        ]
    );
    drop(effect);
    q.finish().await;
}

#[tokio::test]
async fn the_entry_planned_with_more_than_the_threshold_queued_is_bulk_and_other_accounts_are_not()
{
    // Arrange
    let q = Queue::new().await;
    let (other_account, _) = q.second_account().await;
    q.add_many(JobKind::Stills, 100, 5, Priority::Interactive)
        .await;
    let sixth = entry(&q, &Content::new(1, JPEG)).await;
    let seventh = entry(&q, &Content::new(2, JPEG)).await;
    let other_queued = entry_of(&q, other_account, &Content::new(200, JPEG)).await;
    let other = entry_of(&q, other_account, &Content::new(3, JPEG)).await;
    let other_effect = effect(&q, other_account, 1);
    run(&q, &other_effect, other_account, &other_queued)
        .await
        .expect("the other account's first job");

    // Act
    let effect = current(&q);
    run(&q, &effect, q.account, &sixth)
        .await
        .expect("sixth entry");
    run(&q, &effect, q.account, &seventh)
        .await
        .expect("seventh entry");
    run(&q, &other_effect, other_account, &other)
        .await
        .expect("other account's entry");

    // Assert
    assert_eq!(
        jobs(&q, &sixth.content_hash).await,
        [Job::new("stills", "image", 0)]
    );
    assert_eq!(
        jobs(&q, &seventh.content_hash).await,
        [Job::new("stills", "image", 1)]
    );
    assert_eq!(
        jobs_of(&q, other_account, &other.content_hash).await,
        [Job::new("stills", "image", 0)]
    );
    drop((effect, other_effect));
    q.finish().await;
}
