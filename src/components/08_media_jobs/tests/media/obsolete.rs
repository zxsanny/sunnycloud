//! AZ-4332: a job outlives its collected revision (`ON DELETE SET NULL (revision_id)`). Its input and output
//! answer `RevisionObsolete` and the row stays; the skip outcome `obsolete` closes it; the erase hook still
//! removes every job of the account; a new revision of the same content adopts the job and queues it afresh.

use contracts::{AccountLifecycleHook, DerivedKind, WorkerJobError};
use media_api::{EnqueueOutcome, MediaLifecycle, Priority};
use media_shared::types::{JobKind, Lane, SkipOutcome};
use sqlx::Connection;
use uuid::Uuid;

use crate::accept_support::{Accept, WORKER, meta, webp};
use crate::maint_support::{rows_of, seed_jobs};
use crate::queue_support::Queue;
use crate::support::blob;

/// Removes the revision as the retention purge does.
async fn collect(q: &Queue, revision: Uuid) {
    let mut conn = q.fixture.owner_conn().await;
    sqlx::query("DELETE FROM revisions WHERE id = $1")
        .bind(revision)
        .execute(&mut conn)
        .await
        .expect("collect the revision");
    conn.close().await.expect("close owner connection");
}

/// A second revision of the default account's file, over a blob of its own.
async fn another_revision(q: &Queue) -> Uuid {
    let (id, blob_id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = q.fixture.owner_conn().await;
    blob(&mut conn, blob_id, q.account).await;
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
         SELECT $1, account_id, node_id, $3, size, content_hash, blocklist, created_at FROM revisions \
         WHERE id = $2",
    )
    .bind(id)
    .bind(q.revision)
    .bind(blob_id)
    .execute(&mut conn)
    .await
    .expect("another revision");
    conn.close().await.expect("close owner connection");
    id
}

async fn revision_of(q: &Queue, job: Uuid) -> Option<Uuid> {
    let mut conn = q.fixture.owner_conn().await;
    let revision = sqlx::query_scalar("SELECT revision_id FROM jobs WHERE id = $1")
        .bind(job)
        .fetch_one(&mut conn)
        .await
        .expect("job revision");
    conn.close().await.expect("close owner connection");
    revision
}

fn obsolete<T>(result: &Result<T, WorkerJobError>) -> bool {
    matches!(result, Err(WorkerJobError::RevisionObsolete))
}

#[tokio::test]
async fn input_and_output_of_a_collected_revision_are_obsolete_and_the_job_closes_obsolete() {
    // Arrange
    let a = Accept::new().await;
    let job = a.claimed(JobKind::Stills, 40).await;
    collect(&a.q, a.q.revision).await;

    // Act
    let input = a.store.open_input(job, WORKER).await;
    let output = a
        .post(
            job,
            DerivedKind::Thumb,
            meta("image/webp", Some(1), Some(1)),
            webp(),
        )
        .await;
    let open = a.q.row(job).await;
    let closed =
        a.q.queue
            .complete_skipped(job, WORKER, SkipOutcome::Obsolete)
            .await;

    // Assert
    assert!(obsolete(&input), "input");
    assert!(obsolete(&output), "output: {output:?}");
    assert_eq!(
        (open.status.as_str(), revision_of(&a.q, job).await),
        ("running", None)
    );
    closed.expect("the obsolete close");
    let row = a.q.row(job).await;
    assert_eq!(
        (row.status.as_str(), row.outcome.as_deref()),
        ("succeeded", Some("obsolete"))
    );
    assert!(a.derived(40).await.is_empty(), "no derived row");
    a.finish().await;
}

#[tokio::test]
async fn erase_removes_the_jobs_of_collected_and_live_revisions_alike() {
    // Arrange
    let a = Accept::new().await;
    let live = another_revision(&a.q).await;
    let mut conn = a.q.fixture.owner_conn().await;
    seed_jobs(&mut conn, a.q.account, a.q.revision, 8).await;
    conn.close().await.expect("close owner connection");
    a.q.add(JobKind::Poster, 90, Priority::Interactive).await;
    collect(&a.q, a.q.revision).await;
    let mut conn = a.q.fixture.owner_conn().await;
    sqlx::query("UPDATE jobs SET revision_id = $2 WHERE account_id = $1 AND kind = 'poster'")
        .bind(a.q.account)
        .bind(live)
        .execute(&mut conn)
        .await
        .expect("a job of a live revision");
    conn.close().await.expect("close owner connection");
    let hook = MediaLifecycle::new(a.q.fixture.api.clone(), a.gc.clone());

    // Act
    let progress = hook.erase_batch(a.q.account, 50).await.expect("erase");

    // Assert
    assert_eq!(progress.remaining, 0);
    assert_eq!(rows_of(&a.q, a.q.account).await, (0, 0));
    drop(hook);
    a.finish().await;
}

#[tokio::test]
async fn a_new_revision_of_the_same_content_adopts_an_obsolete_job_and_queues_it_afresh() {
    // Arrange
    let q = Queue::new().await;
    let closed = q.add(JobKind::Stills, 2, Priority::Bulk).await;
    let leased = q.queue.claim(Lane::Image, WORKER).await.expect("claim");
    assert_eq!(leased.map(|job| job.job_id), Some(closed));
    let open = q.add(JobKind::Stills, 1, Priority::Bulk).await;
    let new_revision = another_revision(&q).await;
    let kept_spec = q.spec_of(q.account, new_revision, JobKind::Stills, 3);
    assert_eq!(
        q.enqueue(kept_spec, Priority::Bulk).await,
        EnqueueOutcome::Enqueued
    );
    let kept = q.id_of(&kept_spec).await;
    collect(&q, q.revision).await;
    q.queue
        .complete_skipped(closed, WORKER, SkipOutcome::Obsolete)
        .await
        .expect("close obsolete");

    // Act
    let outcomes = [
        q.enqueue(
            q.spec_of(q.account, new_revision, JobKind::Stills, 1),
            Priority::Interactive,
        )
        .await,
        q.enqueue(
            q.spec_of(q.account, new_revision, JobKind::Stills, 2),
            Priority::Interactive,
        )
        .await,
        q.enqueue(
            q.spec_of(q.account, new_revision, JobKind::Stills, 3),
            Priority::Interactive,
        )
        .await,
    ];

    // Assert
    assert_eq!(
        outcomes,
        [
            EnqueueOutcome::Enqueued,
            EnqueueOutcome::Enqueued,
            EnqueueOutcome::AlreadyExists
        ]
    );
    for job in [open, closed] {
        let row = q.row(job).await;
        assert_eq!(
            (
                row.status.as_str(),
                row.attempts,
                row.priority,
                row.leased_by,
                row.outcome
            ),
            ("queued", 0, 0, None, None),
            "{job}"
        );
        assert_eq!(revision_of(&q, job).await, Some(new_revision));
    }
    assert_eq!(
        q.row(kept).await.priority,
        1,
        "a job with its revision is left"
    );
    q.finish().await;
}

#[tokio::test]
async fn an_obsolete_job_of_one_account_is_never_adopted_by_another_accounts_enqueue() {
    // Arrange: account A's job (content 5) lost its revision; account B names the same content.
    let q = Queue::new().await;
    let job_of_a = q.add(JobKind::Stills, 5, Priority::Bulk).await;
    collect(&q, q.revision).await;
    let (account_b, revision_b) = q.second_account().await;

    // Act
    let outcome = q
        .enqueue(
            q.spec_of(account_b, revision_b, JobKind::Stills, 5),
            Priority::Interactive,
        )
        .await;
    let job_of_b = q
        .id_of(&q.spec_of(account_b, revision_b, JobKind::Stills, 5))
        .await;

    // Assert: B gets a job of its own; A's job keeps its state, priority and NULL revision.
    assert_eq!(outcome, EnqueueOutcome::Enqueued);
    assert_ne!(job_of_b, job_of_a);
    assert_eq!(revision_of(&q, job_of_a).await, None);
    assert_eq!(q.row(job_of_a).await.priority, 1);
    assert_eq!(revision_of(&q, job_of_b).await, Some(revision_b));
    q.finish().await;
}
