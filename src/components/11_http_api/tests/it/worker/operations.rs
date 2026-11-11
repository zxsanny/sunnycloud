use contracts::{ClaimedJob, FailResult, JobKind, LeaseExtended, WorkerInput, WorkerJobError};
use hyper::body::Bytes;
use sc_ids::FileHash;
use serde_json::json;

use super::support::{Chunk, JOB, Stack, WORKER, as_worker, job_path};

fn claimed() -> ClaimedJob {
    ClaimedJob {
        job_id: JOB,
        kind: JobKind::Stills,
        content_hash: FileHash::from_bytes([0xAB; 32]),
        params_version: 2,
        attempt: 1,
        lease_ms: 300_000,
    }
}

#[tokio::test]
async fn ac3_claims_answer_200_with_the_job_or_204_and_pass_the_lane_and_worker_id() {
    // Arrange
    let stack = Stack::start().await;
    stack.jobs.push(
        "WorkerJobs::claim",
        Ok::<_, WorkerJobError>(Some(claimed())),
    );
    stack.jobs.push(
        "WorkerJobs::claim",
        Ok::<_, WorkerJobError>(None::<ClaimedJob>),
    );

    // Act
    let some = as_worker(
        stack.internal,
        "POST",
        "/api/v1/worker-jobs/claims",
        &[],
        br#"{"lane":"video"}"#,
    )
    .await;
    let none = as_worker(
        stack.internal,
        "POST",
        "/api/v1/worker-jobs/claims",
        &[],
        br#"{"lane":"image"}"#,
    )
    .await;

    // Assert
    assert_eq!(some.status, 200);
    assert_eq!(
        some.json(),
        json!({
            "job_id": JOB,
            "kind": "stills",
            "content_hash": "ab".repeat(32),
            "params_version": 2,
            "attempt": 1,
            "lease_ms": 300_000,
        })
    );
    assert_eq!(some.header("content-type"), Some("application/json"));
    assert_eq!(none.status, 204);
    assert!(none.body.is_empty());
    assert_eq!(
        stack.jobs.calls(),
        [
            format!("claim Video {WORKER}"),
            format!("claim Image {WORKER}")
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn ac3_lease_put_and_delete_pass_the_job_and_worker_and_map_lease_lost_to_409() {
    // Arrange
    let stack = Stack::start().await;
    let path = job_path("lease");
    stack.jobs.push(
        "WorkerJobs::extend_lease",
        Ok::<_, WorkerJobError>(LeaseExtended { lease_ms: 300_000 }),
    );
    stack.jobs.push(
        "WorkerJobs::extend_lease",
        Err::<LeaseExtended, _>(WorkerJobError::LeaseLost),
    );
    stack
        .jobs
        .push("WorkerJobs::release", Ok::<_, WorkerJobError>(()));
    stack.jobs.push(
        "WorkerJobs::release",
        Err::<(), _>(WorkerJobError::LeaseLost),
    );

    // Act
    let extended = as_worker(stack.internal, "PUT", &path, &[], b"").await;
    let lost_extend = as_worker(stack.internal, "PUT", &path, &[], b"").await;
    let released = as_worker(stack.internal, "DELETE", &path, &[], b"").await;
    let lost_release = as_worker(stack.internal, "DELETE", &path, &[], b"").await;

    // Assert
    assert_eq!(
        (extended.status, extended.json()),
        (200, json!({"lease_ms": 300_000}))
    );
    assert_eq!(released.status, 204);
    for lost in [&lost_extend, &lost_release] {
        assert_eq!((lost.status, lost.code().as_str()), (409, "lease_lost"));
    }
    assert_eq!(
        stack.jobs.calls(),
        [
            format!("extend_lease {JOB} {WORKER}"),
            format!("extend_lease {JOB} {WORKER}"),
            format!("release {JOB} {WORKER}"),
            format!("release {JOB} {WORKER}"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn ac3_input_streams_the_whole_body_with_its_exact_length_and_ignores_range() {
    // Arrange
    let stack = Stack::start().await;
    for _ in 0..2 {
        let input = WorkerInput {
            length: 9,
            body: Box::pin(Chunk(Some(Bytes::from_static(b"plaintext")))),
        };
        stack
            .jobs
            .push("WorkerJobs::open_input", Ok::<_, WorkerJobError>(input));
    }

    // Act
    let plain = as_worker(stack.internal, "GET", &job_path("input"), &[], b"").await;
    let ranged = as_worker(
        stack.internal,
        "GET",
        &job_path("input"),
        &[("range", "bytes=2-4")],
        b"",
    )
    .await;

    // Assert
    for answer in [&plain, &ranged] {
        assert_eq!(answer.status, 200);
        assert_eq!(answer.body, b"plaintext");
        assert_eq!(answer.header("content-length"), Some("9"));
        assert_eq!(
            answer.header("content-type"),
            Some("application/octet-stream")
        );
        assert_eq!(answer.header("cache-control"), Some("no-store"));
        assert!(answer.headers.get("content-range").is_none());
    }
    stack.stop().await;
}

#[tokio::test]
async fn ac3_input_maps_revision_obsolete_to_404_and_lease_lost_to_409() {
    // Arrange
    let stack = Stack::start().await;
    stack.jobs.push(
        "WorkerJobs::open_input",
        Err::<WorkerInput, _>(WorkerJobError::RevisionObsolete),
    );
    stack.jobs.push(
        "WorkerJobs::open_input",
        Err::<WorkerInput, _>(WorkerJobError::LeaseLost),
    );

    // Act
    let obsolete = as_worker(stack.internal, "GET", &job_path("input"), &[], b"").await;
    let lost = as_worker(stack.internal, "GET", &job_path("input"), &[], b"").await;

    // Assert
    assert_eq!(
        (obsolete.status, obsolete.code().as_str()),
        (404, "revision_obsolete")
    );
    assert_eq!((lost.status, lost.code().as_str()), (409, "lease_lost"));
    stack.stop().await;
}

#[tokio::test]
async fn ac3_result_failure_and_status_pass_their_bodies_and_answer_204_200_204() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .jobs
        .push("WorkerJobs::complete_skipped", Ok::<_, WorkerJobError>(()));
    stack.jobs.push(
        "WorkerJobs::fail",
        Ok::<_, WorkerJobError>(FailResult::Requeued),
    );
    stack.jobs.push(
        "WorkerJobs::fail",
        Ok::<_, WorkerJobError>(FailResult::Dead),
    );
    stack
        .jobs
        .push("WorkerJobs::record_status", Ok::<_, WorkerJobError>(()));

    // Act
    let result = as_worker(
        stack.internal,
        "PUT",
        &job_path("result"),
        &[],
        br#"{"outcome":"skipped_compliant"}"#,
    )
    .await;
    let retry = as_worker(
        stack.internal,
        "POST",
        &job_path("failure"),
        &[],
        br#"{"code":"decode","disposition":"retry"}"#,
    )
    .await;
    let dead = as_worker(
        stack.internal,
        "POST",
        &job_path("failure"),
        &[],
        br#"{"code":"oom","disposition":"permanent"}"#,
    )
    .await;
    let status = as_worker(
        stack.internal,
        "PUT",
        "/api/v1/worker-jobs/status",
        &[],
        br#"{"decoder_ok":false,"image_age_days":12}"#,
    )
    .await;

    // Assert
    assert_eq!(result.status, 204);
    assert_eq!(
        (retry.status, retry.json()),
        (200, json!({"result": "requeued"}))
    );
    assert_eq!((dead.status, dead.json()), (200, json!({"result": "dead"})));
    assert_eq!(status.status, 204);
    assert_eq!(
        stack.jobs.calls(),
        [
            format!("complete_skipped {JOB} {WORKER} SkippedCompliant"),
            format!("fail {JOB} {WORKER} decode Retry"),
            format!("fail {JOB} {WORKER} oom Permanent"),
            format!(
                "record_status {WORKER} WorkerStatus {{ decoder_ok: false, image_age_days: 12 }}"
            ),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_call_without_a_worker_id_a_malformed_job_id_or_a_bad_body_is_400_and_calls_nothing() {
    // Arrange
    let stack = Stack::start().await;
    let bearer = super::support::worker_bearer();
    let authorized = [("authorization", bearer.as_str())];

    // Act
    let no_worker = super::support::send(
        stack.internal,
        "POST",
        "/api/v1/worker-jobs/claims",
        &authorized,
        br#"{"lane":"image"}"#,
    )
    .await;
    let bad_job = as_worker(
        stack.internal,
        "PUT",
        "/api/v1/worker-jobs/not-a-uuid/lease",
        &[],
        b"",
    )
    .await;
    let bad_lane = as_worker(
        stack.internal,
        "POST",
        "/api/v1/worker-jobs/claims",
        &[],
        br#"{"lane":"audio"}"#,
    )
    .await;
    let extra = as_worker(
        stack.internal,
        "PUT",
        &job_path("result"),
        &[],
        br#"{"outcome":"obsolete","x":1}"#,
    )
    .await;
    let not_json = as_worker(
        stack.internal,
        "PUT",
        "/api/v1/worker-jobs/status",
        &[],
        b"decoder ok",
    )
    .await;
    let negative = as_worker(
        stack.internal,
        "PUT",
        "/api/v1/worker-jobs/status",
        &[],
        br#"{"decoder_ok":true,"image_age_days":-1}"#,
    )
    .await;

    // Assert
    for answer in [
        &no_worker, &bad_job, &bad_lane, &extra, &not_json, &negative,
    ] {
        assert_eq!(
            (answer.status, answer.code().as_str()),
            (400, "invalid_request")
        );
    }
    assert!(stack.jobs.calls().is_empty(), "{:?}", stack.jobs.calls());
    stack.stop().await;
}

#[tokio::test]
async fn a_port_fault_is_an_opaque_500_and_a_wrong_method_is_405() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let unscripted = as_worker(
        stack.internal,
        "POST",
        "/api/v1/worker-jobs/claims",
        &[],
        br#"{"lane":"image"}"#,
    )
    .await;
    let wrong_method = as_worker(stack.internal, "POST", &job_path("input"), &[], b"").await;
    let head = as_worker(stack.internal, "HEAD", &job_path("input"), &[], b"").await;

    // Assert
    assert_eq!(
        (unscripted.status, unscripted.code().as_str()),
        (500, "internal")
    );
    assert!(!String::from_utf8_lossy(&unscripted.body).contains("unscripted"));
    assert_eq!(wrong_method.status, 405);
    assert_eq!(head.status, 405);
    stack.stop().await;
}
