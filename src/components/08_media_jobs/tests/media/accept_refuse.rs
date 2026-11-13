//! AZ-3842 AC-2 (IT-08-11, AC-25): untrusted worker output is refused by size, sniffed type, kind, dimension
//! bound, revision and lease; a refused output leaves the job as it was, no file and no committed blob row.

use std::sync::Arc;

use contracts::{AcceptOutcome, DerivedKind, TreeReader, WorkerJobError};
use media_shared::types::JobKind;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::accept_doubles::CollectingTree;
use crate::accept_support::{Accept, WORKER, body, meta, mp4, png, webp};
use crate::queue_support::{LEASE, config};

const CAP: u64 = 64 * 1024;

fn label(result: &Result<AcceptOutcome, WorkerJobError>) -> &'static str {
    match result {
        Ok(AcceptOutcome::Accepted) => "Accepted",
        Ok(AcceptOutcome::AlreadyPresent) => "AlreadyPresent",
        Err(WorkerJobError::LeaseLost) => "LeaseLost",
        Err(WorkerJobError::RevisionObsolete) => "RevisionObsolete",
        Err(WorkerJobError::OutputTooLarge) => "OutputTooLarge",
        Err(WorkerJobError::OutputTypeRejected) => "OutputTypeRejected",
        Err(WorkerJobError::OutputKindRejected) => "OutputKindRejected",
        Err(WorkerJobError::Storage(_)) => "Storage",
        Err(WorkerJobError::Unexpected(_)) => "Unexpected",
    }
}

async fn with_cap() -> Accept {
    let mut media = config();
    media.derived_max_thumb = CAP;
    Accept::with(media, |tree| tree, |store| store.registry.clone()).await
}

/// A WebP head padded to `len` bytes.
fn webp_of(len: usize) -> Vec<u8> {
    let mut bytes = webp();
    bytes.resize(len, 0);
    bytes
}

async fn assert_untouched(a: &Accept, jobs: &[Uuid]) {
    for job in jobs {
        let row = a.q.row(*job).await;
        assert_eq!(
            (row.status.as_str(), row.outcome.as_deref()),
            ("running", None)
        );
    }
    assert!(
        a.blobs.files().is_empty(),
        "files kept: {:?}",
        a.blobs.files()
    );
    assert!(a.blob_rows().await.is_empty(), "blob rows kept");
}

#[tokio::test]
async fn oversized_mistyped_wrong_kind_and_out_of_bound_outputs_are_refused_and_nothing_is_kept() {
    // Arrange
    let a = with_cap().await;
    let stills = a.claimed(JobKind::Stills, 20).await;
    let transcode = a.claimed(JobKind::Transcode, 21).await;
    let claimed_webp = || meta("image/webp", Some(320), Some(240));

    // Act
    let mut answers = Vec::new();
    answers.push(
        a.post(
            stills,
            DerivedKind::Thumb,
            claimed_webp(),
            webp_of(65 * 1024),
        )
        .await,
    );
    answers.push(
        a.store
            .accept_output(
                stills,
                WORKER,
                DerivedKind::Thumb,
                claimed_webp(),
                body(webp_of(1_000)),
                34,
            )
            .await,
    );
    answers.push(
        a.post(stills, DerivedKind::Thumb, claimed_webp(), png())
            .await,
    );
    let text = b"this is plain text, not a movie".repeat(10);
    answers.push(
        a.post(
            transcode,
            DerivedKind::Transcode,
            meta("video/mp4", None, None),
            text,
        )
        .await,
    );
    answers.push(
        a.post(stills, DerivedKind::Thumb, claimed_webp(), mp4(100))
            .await,
    );
    answers.push(
        a.post(
            stills,
            DerivedKind::Transcode,
            meta("video/mp4", None, None),
            mp4(100),
        )
        .await,
    );
    answers.push(
        a.post(
            stills,
            DerivedKind::Thumb,
            meta("image/webp", Some(321), None),
            webp(),
        )
        .await,
    );

    // Assert
    let labels: Vec<&str> = answers.iter().map(label).collect();
    assert_eq!(
        labels,
        [
            "OutputTooLarge",
            "OutputTooLarge",
            "OutputTypeRejected",
            "OutputTypeRejected",
            "OutputTypeRejected",
            "OutputKindRejected",
            "OutputTypeRejected",
        ]
    );
    assert!(a.derived(20).await.is_empty() && a.derived(21).await.is_empty());
    assert_untouched(&a, &[stills, transcode]).await;
    a.finish().await;
}

#[tokio::test]
async fn an_expired_lease_and_another_worker_are_lease_lost() {
    // Arrange
    let a = with_cap().await;
    let stills = a.claimed(JobKind::Stills, 22).await;
    let other = a
        .store
        .accept_output(
            stills,
            "w2",
            DerivedKind::Thumb,
            meta("image/webp", None, None),
            body(webp()),
            34,
        )
        .await;
    a.q.clock.tick(LEASE);

    // Act
    let expired = a
        .post(
            stills,
            DerivedKind::Thumb,
            meta("image/webp", None, None),
            webp(),
        )
        .await;

    // Assert
    assert_eq!([label(&other), label(&expired)], ["LeaseLost", "LeaseLost"]);
    assert_eq!(a.q.row(stills).await.status, "running");
    assert!(a.blobs.files().is_empty());
    assert!(a.blob_rows().await.is_empty());
    a.finish().await;
}

#[tokio::test]
async fn a_revision_collected_after_the_holder_check_is_obsolete_and_one_collected_before_takes_the_job_with_it()
 {
    // Arrange
    let mut collecting: Option<Arc<CollectingTree>> = None;
    let a = Accept::with(
        config(),
        |tree| {
            let wrapped = Arc::new(CollectingTree {
                inner: tree,
                owner: Mutex::new(None),
            });
            collecting = Some(wrapped.clone());
            wrapped as Arc<dyn TreeReader>
        },
        |store| store.registry.clone(),
    )
    .await;
    let collecting = collecting.expect("the tree wrapper");
    let stills = a.claimed(JobKind::Stills, 23).await;
    collecting.arm(a.q.fixture.owner_conn().await).await;

    // Act
    let during = a
        .post(
            stills,
            DerivedKind::Thumb,
            meta("image/webp", None, None),
            webp(),
        )
        .await;
    let after = a
        .post(
            stills,
            DerivedKind::Thumb,
            meta("image/webp", None, None),
            webp(),
        )
        .await;

    // Assert
    assert_eq!(
        [label(&during), label(&after)],
        ["RevisionObsolete", "LeaseLost"]
    );
    let jobs = a.q.count("SELECT count(*) FROM jobs").await;
    assert_eq!(
        jobs, 0,
        "the collected revision's job is removed by the cascade"
    );
    assert!(a.blobs.files().is_empty());
    // Only the collected revision's own blob row is left unreferenced (its removal is the purge's, not 08's).
    assert_eq!(a.blob_rows().await.len(), 1);
    assert!(a.derived(23).await.is_empty());
    collecting.close().await;
    drop(collecting);
    a.finish().await;
}
