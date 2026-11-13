//! AZ-3842 AC-71 (IT-08-10) and AC-4: one derived file per key after a repeat, a race, a partial retry or a fault
//! between the file write and the row transaction; surplus files are queued `Due::Now` and removed by 04's GC.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use contracts::{AcceptOutcome, DerivedKind, WorkerJobError};
use media_shared::types::{JobKind, Lane};

use crate::accept_doubles::FailingRegistry;
use crate::accept_support::{Accept, WORKER, body, meta, webp};
use crate::queue_support::{LEASE, config};

/// The surplus rows: state `gc_pending`, reason `derived_removed`, due now.
fn surplus() -> (String, Option<String>, Option<bool>) {
    (
        "gc_pending".to_owned(),
        Some("derived_removed".to_owned()),
        Some(true),
    )
}

fn kept() -> (String, Option<String>, Option<bool>) {
    ("committed".to_owned(), None, None)
}

#[tokio::test]
async fn a_repeated_thumb_is_already_present_and_its_surplus_file_is_queued_now() {
    // Arrange
    let a = Accept::new().await;
    let stills = a.claimed(JobKind::Stills, 30).await;

    // Act
    let first = a.post_webp(stills, DerivedKind::Thumb).await;
    let second = a.post_webp(stills, DerivedKind::Thumb).await;

    // Assert
    assert_eq!(
        (first, second),
        (AcceptOutcome::Accepted, AcceptOutcome::AlreadyPresent)
    );
    assert_eq!(a.q.row(stills).await.status, "running");
    let rows = a.derived(30).await;
    assert_eq!(rows.len(), 1);
    let mut states: Vec<_> = a
        .blob_rows()
        .await
        .into_iter()
        .map(|row| (row.1, row.2, row.3))
        .collect();
    states.sort();
    assert_eq!(states, [kept(), surplus()]);
    a.finish().await;
}

#[tokio::test]
async fn of_eight_racing_posts_one_is_accepted_and_gc_leaves_only_the_referenced_file() {
    // Arrange
    let a = Accept::new().await;
    let poster = a.claimed(JobKind::Poster, 31).await;

    // Act
    let racers: Vec<_> = (0..8)
        .map(|_| {
            let store = a.store.clone();
            tokio::spawn(async move {
                store
                    .accept_output(
                        poster,
                        WORKER,
                        DerivedKind::Thumb,
                        meta("image/webp", None, None),
                        body(webp()),
                        34,
                    )
                    .await
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for racer in racers {
        outcomes.push(racer.await.expect("task").expect("answered"));
    }
    let lost_answer_retry = a.post_webp(poster, DerivedKind::Thumb).await;
    let queued: Vec<_> = a.blob_rows().await;
    let removed = a.sweep().await;

    // Assert
    let accepted = outcomes
        .iter()
        .filter(|o| **o == AcceptOutcome::Accepted)
        .count();
    let present = outcomes
        .iter()
        .filter(|o| **o == AcceptOutcome::AlreadyPresent)
        .count();
    assert_eq!((accepted, present), (1, 7));
    assert_eq!(lost_answer_retry, AcceptOutcome::AlreadyPresent);
    let row = a.q.row(poster).await;
    assert_eq!(
        (row.status.as_str(), row.outcome.as_deref()),
        ("succeeded", Some("derived"))
    );
    let rows = a.derived(31).await;
    assert_eq!(rows.len(), 1);
    let surplus_count = queued
        .iter()
        .filter(|row| (row.1.clone(), row.2.clone(), row.3) == surplus())
        .count();
    assert_eq!((queued.len(), surplus_count), (9, 8));
    assert_eq!(removed, 8);
    assert_eq!(a.blobs.files(), BTreeSet::from([rows[0].blob_id]));
    let left: Vec<_> = a.blob_rows().await.into_iter().map(|row| row.0).collect();
    assert_eq!(left, [rows[0].blob_id]);
    a.finish().await;
}

#[tokio::test]
async fn a_stills_retry_with_the_preview_stored_gets_already_present_then_accepted_and_closes() {
    // Arrange
    let a = Accept::new().await;
    let stills = a.claimed(JobKind::Stills, 32).await;
    assert_eq!(
        a.post_webp(stills, DerivedKind::Preview).await,
        AcceptOutcome::Accepted
    );
    a.q.clock.tick(LEASE);
    let retried =
        a.q.queue
            .claim(Lane::Image, WORKER)
            .await
            .expect("claim")
            .expect("the expired job");
    assert_eq!((retried.job_id, retried.attempt), (stills, 2));

    // Act
    let preview = a.post_webp(stills, DerivedKind::Preview).await;
    let thumb = a.post_webp(stills, DerivedKind::Thumb).await;

    // Assert
    assert_eq!(
        (preview, thumb),
        (AcceptOutcome::AlreadyPresent, AcceptOutcome::Accepted)
    );
    let row = a.q.row(stills).await;
    assert_eq!(
        (row.status.as_str(), row.outcome.as_deref()),
        ("succeeded", Some("derived"))
    );
    let kinds: Vec<String> = a
        .derived(32)
        .await
        .into_iter()
        .map(|row| row.kind)
        .collect();
    assert_eq!(kinds, ["preview", "thumb"]);
    a.finish().await;
}

#[tokio::test]
async fn a_fault_after_the_file_write_leaves_no_row_and_the_retry_is_accepted() {
    // Arrange
    let a = Accept::with(
        config(),
        |tree| tree,
        |store| {
            Arc::new(FailingRegistry {
                inner: store.registry.clone(),
                fail_next: AtomicBool::new(true),
            })
        },
    )
    .await;
    let poster = a.claimed(JobKind::Poster, 33).await;

    // Act
    let faulted = a
        .post(
            poster,
            DerivedKind::Thumb,
            meta("image/webp", None, None),
            webp(),
        )
        .await;
    let rows_after_fault = a.derived(33).await.len();
    let files_after_fault = a.blobs.files().len();
    let retry = a.post_webp(poster, DerivedKind::Thumb).await;

    // Assert
    assert!(
        matches!(faulted, Err(WorkerJobError::Unexpected(_))),
        "{faulted:?}"
    );
    assert_eq!((rows_after_fault, files_after_fault), (0, 0));
    assert_eq!(retry, AcceptOutcome::Accepted);
    let rows = a.derived(33).await;
    assert_eq!(a.blobs.files(), BTreeSet::from([rows[0].blob_id]));
    assert_eq!(a.q.row(poster).await.status, "succeeded");
    a.finish().await;
}
