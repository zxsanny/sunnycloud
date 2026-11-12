//! AZ-3841 AC-4, the `media-job-rollback` case of the tree-commit contract: the real `MediaEnqueueEffect` inside the
//! real 02 `TreeCommit::publish`. A job exists only if its entry's savepoint and the commit do.

use std::collections::HashSet;
use std::sync::Arc;

use contracts::{
    CommitEffect, EntryFailure, EntryResult, PublishResult, TreeCommit, TreeCommitError,
};
use sc_error::ErrorCode;

use crate::effect_support::{Job, current, jobs};
use crate::publish_support::{Failing, actor, new_photo, tree_service};
use crate::queue_support::{Queue, hash};

#[tokio::test]
async fn an_entry_that_fails_after_the_media_effect_leaves_no_job_and_the_others_keep_theirs() {
    // Arrange
    let q = Queue::new().await;
    let (service, root) = tree_service(&q).await;
    let entries = vec![
        new_photo(&q, root, "p1.jpg", hash(1)).await,
        new_photo(&q, root, "p2.jpg", hash(2)).await,
        new_photo(&q, root, "p3.jpg", hash(3)).await,
    ];
    let failing = Failing {
        entries: HashSet::from([entries[1].revision.revision_id]),
        batch: false,
    };
    let effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(current(&q)), Arc::new(failing)];

    // Act
    let result = service
        .publish(actor(q.account), entries, effects, sc_testdb::START)
        .await
        .expect("the batch commits");

    // Assert
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay");
    };
    assert!(matches!(results[0], EntryResult::Published { .. }));
    assert_eq!(
        results[1],
        EntryResult::Failed(EntryFailure::EffectFailed(ErrorCode::UploadSessionGone))
    );
    assert!(matches!(results[2], EntryResult::Published { .. }));
    let stills = vec![Job::new("stills", "image", 0)];
    assert_eq!(jobs(&q, &hash(1)).await, stills);
    assert_eq!(jobs(&q, &hash(2)).await, []);
    assert_eq!(jobs(&q, &hash(3)).await, stills);
    drop(service);
    q.finish().await;
}

#[tokio::test]
async fn a_rolled_back_commit_leaves_no_job() {
    // Arrange
    let q = Queue::new().await;
    let (service, root) = tree_service(&q).await;
    let entries = vec![new_photo(&q, root, "p1.jpg", hash(1)).await];
    let failing = Failing {
        entries: HashSet::new(),
        batch: true,
    };
    let effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(current(&q)), Arc::new(failing)];

    // Act
    let result = service
        .publish(actor(q.account), entries, effects, sc_testdb::START)
        .await;

    // Assert
    assert!(
        matches!(
            result,
            Err(TreeCommitError::EffectFailed(ErrorCode::Internal))
        ),
        "{result:?}"
    );
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 0);
    drop(service);
    q.finish().await;
}
