//! AZ-3843 AC-2 (IT-08-6, ST-08-2 port part) and AC-4 (ST-08-2): the `WorkerJobs` facade over the real queue and
//! the real derived store. A worker that does not hold the job gets `LeaseLost` on every job-scoped operation, no
//! input byte and no state change; the holder's calls reach the queue and the store; a claim carries the job
//! fields only.

use std::sync::Arc;

use contracts::{
    AcceptOutcome, ClaimedJob, DerivedKind, FailDisposition, FailResult, JobKind, Lane,
    SkipOutcome, WorkerJobError, WorkerJobs,
};
use media_api::{Priority, WorkerJobsFacade};
use media_shared::types::JobKind as WireKind;

use crate::accept_input::{content, hashes, new_at, publish, read_all};
use crate::accept_support::{Accept, WORKER, body, meta, webp};
use crate::publish_support::tree_service;
use crate::queue_support::{BACKOFF, LEASE, hash};

const INTRUDER: &str = "w2";

fn facade(a: &Accept) -> WorkerJobsFacade {
    WorkerJobsFacade::new(
        a.q.fixture.api.clone(),
        a.q.clock.clone(),
        a.q.queue.clone(),
        Arc::clone(&a.store),
    )
}

fn label<T>(result: &Result<T, WorkerJobError>) -> &'static str {
    match result {
        Ok(_) => "Ok",
        Err(WorkerJobError::LeaseLost) => "LeaseLost",
        Err(_) => "other",
    }
}

#[tokio::test]
async fn a_worker_that_does_not_hold_the_job_gets_lease_lost_everywhere_no_input_byte_and_no_change()
 {
    // Arrange
    let a = Accept::new().await;
    let jobs = facade(&a);
    let (service, root) = tree_service(&a.q).await;
    let photo = content(500_000, 5);
    publish(
        &a,
        &service,
        new_at(root, "photo.jpg"),
        &photo,
        "image/jpeg",
    )
    .await;
    let held = jobs
        .claim(Lane::Image, WORKER)
        .await
        .expect("claim")
        .expect("the photo's job");
    let id = held.job_id;
    let before = format!("{:?}", a.q.row(id).await);
    let thumb = webp();
    let length = u64::try_from(thumb.len()).expect("length");

    // Act
    let calls = [
        label(&jobs.extend_lease(id, INTRUDER).await),
        label(&jobs.release(id, INTRUDER).await),
        label(
            &jobs
                .complete_skipped(id, INTRUDER, SkipOutcome::Obsolete)
                .await,
        ),
        label(
            &jobs
                .fail(id, INTRUDER, "decode_failed", FailDisposition::Permanent)
                .await,
        ),
        label(&jobs.open_input(id, INTRUDER).await),
        label(
            &jobs
                .accept_output(
                    id,
                    INTRUDER,
                    DerivedKind::Thumb,
                    meta("image/webp", Some(1), Some(1)),
                    body(thumb),
                    length,
                )
                .await,
        ),
    ];

    // Assert
    assert_eq!(calls, ["LeaseLost"; 6]);
    assert_eq!(format!("{:?}", a.q.row(id).await), before, "job state");
    assert_eq!(a.q.count("SELECT count(*) FROM derived").await, 0);
    assert!(a.blob_rows().await.is_empty(), "no output blob was written");
    let (length, bytes) = read_all(jobs.open_input(id, WORKER).await.expect("holder input")).await;
    assert_eq!(length, u64::try_from(photo.len()).expect("len"));
    assert_eq!(
        hashes(&bytes).1,
        held.content_hash,
        "the holder still reads the input"
    );
    drop((service, jobs));
    a.finish().await;
}

#[tokio::test]
async fn a_claim_carries_only_the_job_fields() {
    // Arrange
    let a = Accept::new().await;
    let jobs = facade(&a);
    let id = a.q.add(WireKind::Poster, 4, Priority::Interactive).await;

    // Act
    let claimed = jobs.claim(Lane::Video, WORKER).await.expect("claim");

    // Assert
    // Exhaustive: a new field (an account, a path, a name) would not compile here.
    let Some(ClaimedJob {
        job_id,
        kind,
        content_hash,
        params_version,
        attempt,
        lease_ms,
    }) = claimed
    else {
        panic!("no job claimed");
    };
    assert_eq!(
        (
            job_id,
            kind,
            content_hash,
            params_version,
            attempt,
            lease_ms
        ),
        (id, JobKind::Poster, hash(4), 1, 1, LEASE.as_millis() as u64)
    );
    assert!(
        jobs.claim(Lane::Video, WORKER)
            .await
            .expect("claim")
            .is_none(),
        "an empty lane answers None"
    );
    drop(jobs);
    a.finish().await;
}

#[tokio::test]
async fn the_holders_calls_reach_the_queue_and_the_store() {
    // Arrange
    let a = Accept::new().await;
    let jobs = facade(&a);
    let stills = a.q.add(WireKind::Stills, 1, Priority::Interactive).await;
    let transcode = a.q.add(WireKind::Transcode, 2, Priority::Interactive).await;
    let thumb = webp();
    let length = u64::try_from(thumb.len()).expect("length");

    // Act
    let first = jobs.claim(Lane::Image, WORKER).await.expect("claim");
    let extended = jobs.extend_lease(stills, WORKER).await.expect("extend");
    let accepted = jobs
        .accept_output(
            stills,
            WORKER,
            DerivedKind::Thumb,
            meta("image/webp", Some(1), Some(1)),
            body(thumb),
            length,
        )
        .await
        .expect("output");
    let failed = jobs
        .fail(stills, WORKER, "decode_failed", FailDisposition::Retry)
        .await
        .expect("fail");
    a.q.clock.tick(BACKOFF);
    let again = jobs.claim(Lane::Image, WORKER).await.expect("claim");
    jobs.complete_skipped(stills, WORKER, SkipOutcome::SkippedCompliant)
        .await
        .expect("skip");
    let video = jobs.claim(Lane::Video, WORKER).await.expect("claim");
    jobs.release(transcode, WORKER).await.expect("release");

    // Assert
    assert_eq!(
        first.map(|job| (job.job_id, job.attempt)),
        Some((stills, 1))
    );
    assert_eq!(extended.lease_ms, LEASE.as_millis() as u64);
    assert_eq!(accepted, AcceptOutcome::Accepted);
    assert_eq!(failed, FailResult::Requeued);
    assert_eq!(
        again.map(|job| (job.job_id, job.attempt)),
        Some((stills, 2))
    );
    assert_eq!(video.map(|job| job.job_id), Some(transcode));
    let closed = a.q.row(stills).await;
    assert_eq!(
        (closed.status.as_str(), closed.outcome.as_deref()),
        ("succeeded", Some("skipped_compliant"))
    );
    let released = a.q.row(transcode).await;
    assert_eq!((released.status.as_str(), released.attempts), ("queued", 0));
    drop(jobs);
    a.finish().await;
}
