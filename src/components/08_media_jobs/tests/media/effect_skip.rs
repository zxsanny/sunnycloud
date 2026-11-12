//! IT-08-2 (AZ-3841 AC-2): enqueue through the effect is idempotent and skips a job whose outputs all exist.

use crate::effect_support::{
    Content, JPEG, Job, MP4, add_derived, current, effect, entry, jobs, run,
};
use crate::queue_support::Queue;

#[tokio::test]
async fn the_same_entry_twice_and_a_second_revision_of_the_content_leave_one_job() {
    // Arrange
    let q = Queue::new().await;
    let effect = current(&q);
    let content = Content::new(1, JPEG);
    let first = entry(&q, &content).await;
    let second_revision = entry(&q, &content).await;

    // Act
    for published in [&first, &first, &second_revision] {
        run(&q, &effect, q.account, published)
            .await
            .expect("the effect runs");
    }

    // Assert
    assert_eq!(
        jobs(&q, &content.hash).await,
        [Job::new("stills", "image", 0)]
    );
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 1);
    drop(effect);
    q.finish().await;
}

#[tokio::test]
async fn a_stills_job_is_enqueued_when_only_the_thumb_exists() {
    // Arrange
    let q = Queue::new().await;
    let content = Content::new(1, JPEG);
    add_derived(&q, &content.hash, "thumb").await;
    let published = entry(&q, &content).await;

    // Act
    run(&q, &current(&q), q.account, &published)
        .await
        .expect("the effect runs");

    // Assert
    assert_eq!(
        jobs(&q, &content.hash).await,
        [Job::new("stills", "image", 0)]
    );
    q.finish().await;
}

#[tokio::test]
async fn nothing_is_enqueued_when_every_output_exists() {
    // Arrange
    let q = Queue::new().await;
    let content = Content::new(1, JPEG);
    add_derived(&q, &content.hash, "thumb").await;
    add_derived(&q, &content.hash, "preview").await;
    let published = entry(&q, &content).await;

    // Act
    run(&q, &current(&q), q.account, &published)
        .await
        .expect("the effect runs");

    // Assert
    assert_eq!(jobs(&q, &content.hash).await, []);
    q.finish().await;
}

#[tokio::test]
async fn a_video_whose_poster_thumb_exists_gets_only_the_transcode_job() {
    // Arrange
    let q = Queue::new().await;
    let content = Content::new(1, MP4);
    add_derived(&q, &content.hash, "thumb").await;
    let published = entry(&q, &content).await;

    // Act
    run(&q, &current(&q), q.account, &published)
        .await
        .expect("the effect runs");

    // Assert
    assert_eq!(
        jobs(&q, &content.hash).await,
        [Job::new("transcode", "video", 0)]
    );
    q.finish().await;
}

#[tokio::test]
async fn a_new_params_version_enqueues_a_new_job_for_the_same_content() {
    // Arrange
    let q = Queue::new().await;
    let content = Content::new(1, JPEG);
    add_derived(&q, &content.hash, "thumb").await;
    add_derived(&q, &content.hash, "preview").await;
    let published = entry(&q, &content).await;
    run(&q, &current(&q), q.account, &published)
        .await
        .expect("the effect runs at version 1");

    // Act
    run(&q, &effect(&q, q.account, 2), q.account, &published)
        .await
        .expect("the effect runs at version 2");

    // Assert
    assert_eq!(
        jobs(&q, &content.hash).await,
        [Job {
            params_version: 2,
            ..Job::new("stills", "image", 0)
        }]
    );
    q.finish().await;
}
