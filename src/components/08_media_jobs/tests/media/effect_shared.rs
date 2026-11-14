//! AZ-4311 AC-1: one shared `MediaEnqueueEffect` serves every account. Each entry is planned under the account it
//! carries (`PublishedEntry::account_id`) and runs in that account's own transaction.

use crate::effect_support::{Content, JPEG, Job, current, entry_of, jobs_of, run};
use crate::queue_support::Queue;

#[tokio::test]
async fn one_shared_effect_queues_each_accounts_jpeg_under_its_own_account() {
    // Arrange
    let q = Queue::new().await;
    let (other_account, _) = q.second_account().await;
    let effect = current(&q);
    let first = entry_of(&q, q.account, &Content::new(1, JPEG)).await;
    let second = entry_of(&q, other_account, &Content::new(2, JPEG)).await;

    // Act
    run(&q, &effect, &first)
        .await
        .expect("the first account's entry");
    run(&q, &effect, &second)
        .await
        .expect("the second account's entry");

    // Assert
    let stills = [Job::new("stills", "image", 0)];
    assert_eq!(jobs_of(&q, q.account, &first.content_hash).await, stills);
    assert_eq!(
        jobs_of(&q, other_account, &second.content_hash).await,
        stills
    );
    assert_eq!(jobs_of(&q, q.account, &second.content_hash).await, []);
    assert_eq!(jobs_of(&q, other_account, &first.content_hash).await, []);
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 2);
    drop(effect);
    q.finish().await;
}
