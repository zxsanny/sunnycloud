//! AC-1: enqueue is idempotent on (account, kind, content hash, params version) and joins the caller's
//! account-scoped transaction.

use media_api::{EnqueueOutcome, Priority};
use media_shared::types::JobKind;
use sc_db::{DbError, Scope};

use crate::queue_support::Queue;

#[tokio::test]
async fn the_same_spec_twice_is_one_job_and_another_params_version_is_a_second() {
    // Arrange
    let q = Queue::new().await;
    let spec = q.spec(JobKind::Stills, 1);
    let mut next_version = spec;
    next_version.params_version = 2;

    // Act
    let first = q.enqueue(spec, Priority::Interactive).await;
    let second = q.enqueue(spec, Priority::Bulk).await;
    let third = q.enqueue(next_version, Priority::Interactive).await;

    // Assert
    assert_eq!(first, EnqueueOutcome::Enqueued);
    assert_eq!(second, EnqueueOutcome::AlreadyExists);
    assert_eq!(third, EnqueueOutcome::Enqueued);
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 2);
    let kept = q.row(q.id_of(&spec).await).await;
    assert_eq!(
        (kept.status.as_str(), kept.priority, kept.attempts),
        ("queued", 0, 0)
    );
    q.finish().await;
}

#[tokio::test]
async fn another_kind_or_account_with_the_same_hash_is_a_separate_job() {
    // Arrange
    let q = Queue::new().await;
    let (other_account, other_revision) = q.second_account().await;

    // Act
    let stills = q
        .enqueue(q.spec(JobKind::Stills, 1), Priority::Interactive)
        .await;
    let poster = q
        .enqueue(q.spec(JobKind::Poster, 1), Priority::Interactive)
        .await;
    let foreign = q
        .enqueue(
            q.spec_of(other_account, other_revision, JobKind::Stills, 1),
            Priority::Interactive,
        )
        .await;

    // Assert
    assert_eq!([stills, poster, foreign], [EnqueueOutcome::Enqueued; 3]);
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 3);
    q.finish().await;
}

#[tokio::test]
async fn a_job_exists_only_if_the_callers_transaction_commits() {
    // Arrange
    let q = Queue::new().await;
    let spec = q.spec(JobKind::Stills, 7);

    // Act
    let rolled_back = q
        .fixture
        .api
        .run(Scope::Account(q.account), async |tx| {
            q.queue
                .enqueue_in_tx(tx, spec, Priority::Interactive)
                .await?;
            Err::<(), _>(DbError::Other("caller failed".into()))
        })
        .await;

    // Assert
    assert!(rolled_back.is_err());
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 0);
    q.finish().await;
}

#[tokio::test]
async fn enqueue_in_the_scope_of_another_account_is_refused_by_the_row_policy() {
    // Arrange
    let q = Queue::new().await;
    let (other_account, _) = q.second_account().await;
    let spec = q.spec(JobKind::Stills, 1);

    // Act
    let refused = q
        .fixture
        .api
        .run(Scope::Account(other_account), async |tx| {
            q.queue.enqueue_in_tx(tx, spec, Priority::Interactive).await
        })
        .await;

    // Assert
    let message = format!("{refused:?}");
    assert!(message.contains("42501"), "{message}");
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 0);
    q.finish().await;
}
