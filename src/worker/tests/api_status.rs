//! AZ-3869 AC-1: every route's documented status becomes one typed result; a status that a route does not
//! document is never read as another route's meaning.

mod api_support;

use api_support::{Peer, Reply, Scratch, client, file_hash, job, pattern, refused_url};
use media_shared::types::{
    ClaimedJob, DerivedKind, Disposition, FailResult, FailureReport, JobKind, Lane, OutputMeta,
    SkipOutcome, WorkerStatus,
};
use sunnycloud_worker::ApiError;
use uuid::Uuid;

type FetchCheck = fn(&ApiError) -> bool;
type PostCheck = fn(&Result<(), ApiError>) -> bool;

const JOB_ID: Uuid = Uuid::from_u128(0x5eed);

fn claimed() -> ClaimedJob {
    job(JobKind::Stills, file_hash(&pattern(10)))
}

fn claimed_json() -> String {
    serde_json::to_string(&claimed()).expect("job json")
}

fn meta() -> OutputMeta {
    OutputMeta {
        media_type: "image/webp".into(),
        width: Some(1440),
        height: Some(960),
        duration_ms: None,
    }
}

fn failure() -> FailureReport {
    FailureReport {
        code: "corrupt".into(),
        disposition: Disposition::Permanent,
    }
}

async fn post(api: &sunnycloud_worker::WorkerApiClient) -> Result<(), ApiError> {
    let scratch = Scratch::new(JOB_ID);
    scratch.write_output("webp", b"webp bytes");
    let output = scratch.dir.open_output("webp", 1 << 20).expect("output");
    api.post_output(JOB_ID, DerivedKind::Preview, &meta(), output)
        .await
}

#[tokio::test]
async fn claim_gives_the_job_for_200_and_none_for_204() {
    // Arrange
    let busy = Peer::start(|_| Reply::json(200, &claimed_json())).await;
    let idle = Peer::start(|_| Reply::status(204)).await;

    // Act
    let job = client(&busy.base_url()).claim(Lane::Image).await;
    let none = client(&idle.base_url()).claim(Lane::Video).await;

    // Assert
    assert_eq!(job.expect("claim answered"), Some(claimed()));
    assert_eq!(none.expect("claim answered"), None);
    let seen = busy.requests();
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/api/v1/worker-jobs/claims")
    );
    assert_eq!(seen[0].body, br#"{"lane":"image"}"#);
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    assert_eq!(idle.requests()[0].body, br#"{"lane":"video"}"#);
}

#[tokio::test]
async fn heartbeat_extends_for_200_and_reports_a_lost_lease_for_409() {
    // Arrange
    let held = Peer::start(|_| Reply::json(200, r#"{"lease_ms":90000}"#)).await;
    let lost = Peer::start(|_| Reply::status(409)).await;

    // Act
    let extended = client(&held.base_url()).heartbeat(JOB_ID).await;
    let refused = client(&lost.base_url()).heartbeat(JOB_ID).await;

    // Assert
    assert_eq!(extended.expect("extended").lease_ms, 90_000);
    assert!(matches!(refused, Err(ApiError::LeaseLost)), "{refused:?}");
    let seen = held.requests();
    assert_eq!(seen[0].method, "PUT");
    assert_eq!(seen[0].path, format!("/api/v1/worker-jobs/{JOB_ID}/lease"));
}

#[tokio::test]
async fn release_complete_and_status_succeed_on_204_and_send_their_bodies() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(204)).await;
    let api = client(&peer.base_url());

    // Act
    let released = api.release(JOB_ID).await;
    let completed = api.complete(JOB_ID, SkipOutcome::SkippedCompliant).await;
    let closed = api.complete(JOB_ID, SkipOutcome::Obsolete).await;
    let reported = api
        .report_status(&WorkerStatus {
            decoder_ok: true,
            image_age_days: 12,
        })
        .await;

    // Assert
    for result in [released, completed, closed, reported] {
        assert!(result.is_ok(), "{result:?}");
    }
    let seen = peer.requests();
    let line = |i: usize| (seen[i].method.as_str(), seen[i].path.clone());
    assert_eq!(
        line(0),
        ("DELETE", format!("/api/v1/worker-jobs/{JOB_ID}/lease"))
    );
    assert_eq!(
        line(1),
        ("PUT", format!("/api/v1/worker-jobs/{JOB_ID}/result"))
    );
    assert_eq!(seen[1].body, br#"{"outcome":"skipped_compliant"}"#);
    assert_eq!(seen[2].body, br#"{"outcome":"obsolete"}"#);
    assert_eq!(line(3), ("PUT", "/api/v1/worker-jobs/status".to_owned()));
    assert_eq!(seen[3].body, br#"{"decoder_ok":true,"image_age_days":12}"#);
}

#[tokio::test]
async fn fail_returns_requeued_or_dead_as_the_api_answers() {
    // Arrange
    let requeue = Peer::start(|_| Reply::json(200, r#"{"result":"requeued"}"#)).await;
    let dead = Peer::start(|_| Reply::json(200, r#"{"result":"dead"}"#)).await;

    // Act
    let first = client(&requeue.base_url()).fail(JOB_ID, &failure()).await;
    let second = client(&dead.base_url()).fail(JOB_ID, &failure()).await;

    // Assert
    assert_eq!(first.expect("answered"), FailResult::Requeued);
    assert_eq!(second.expect("answered"), FailResult::Dead);
    let seen = requeue.requests();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        seen[0].path,
        format!("/api/v1/worker-jobs/{JOB_ID}/failure")
    );
    assert_eq!(
        seen[0].body,
        br#"{"code":"corrupt","disposition":"permanent"}"#
    );
}

#[tokio::test]
async fn fetch_maps_404_409_and_403_and_leaves_the_destination_empty() {
    // Arrange
    let cases: [(u16, FetchCheck); 3] = [
        (404, |e| matches!(e, ApiError::Gone)),
        (409, |e| matches!(e, ApiError::LeaseLost)),
        (403, |e| matches!(e, ApiError::Forbidden)),
    ];

    for (status, expected) in cases {
        let peer = Peer::start(move |_| Reply::status(status)).await;
        let scratch = Scratch::new(JOB_ID);

        // Act
        let result = client(&peer.base_url())
            .fetch_input(&claimed(), scratch.source())
            .await;

        // Assert
        let error = result.expect_err("fetch refused");
        assert!(expected(&error), "{status} gave {error:?}");
        assert_eq!(scratch.source_len(), 0, "{status}");
    }
}

#[tokio::test]
async fn post_maps_204_and_200_to_success_and_413_422_409_404_to_their_errors() {
    // Arrange
    let cases: [(u16, PostCheck); 6] = [
        (204, |r| r.is_ok()),
        (200, |r| r.is_ok()),
        (413, |r| matches!(r, Err(ApiError::TooLarge))),
        (422, |r| matches!(r, Err(ApiError::TypeRejected))),
        (409, |r| matches!(r, Err(ApiError::LeaseLost))),
        (404, |r| matches!(r, Err(ApiError::Gone))),
    ];

    for (status, expected) in cases {
        let peer = Peer::start(move |_| Reply::status(status)).await;

        // Act
        let result = post(&client(&peer.base_url())).await;

        // Assert
        assert!(expected(&result), "{status} gave {result:?}");
    }
}

#[tokio::test]
async fn a_503_gives_transient_on_every_call() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(503)).await;
    let api = client(&peer.base_url());

    // Act
    let results = [
        api.claim(Lane::Image).await.map(drop),
        api.heartbeat(JOB_ID).await.map(drop),
        api.release(JOB_ID).await,
        api.complete(JOB_ID, SkipOutcome::Obsolete).await,
        api.fail(JOB_ID, &failure()).await.map(drop),
        api.report_status(&WorkerStatus {
            decoder_ok: false,
            image_age_days: 0,
        })
        .await,
        api.fetch_input(&claimed(), Scratch::new(JOB_ID).source())
            .await
            .map(drop),
        post(&api).await,
    ];

    // Assert
    for result in results {
        assert!(matches!(result, Err(ApiError::Transient(_))), "{result:?}");
    }
}

#[tokio::test]
async fn a_refused_connection_gives_transient_on_every_call() {
    // Arrange
    let api = client(&refused_url().await);

    // Act
    let results = [
        api.claim(Lane::Image).await.map(drop),
        api.heartbeat(JOB_ID).await.map(drop),
        api.release(JOB_ID).await,
        api.complete(JOB_ID, SkipOutcome::Obsolete).await,
        api.fail(JOB_ID, &failure()).await.map(drop),
        api.report_status(&WorkerStatus {
            decoder_ok: false,
            image_age_days: 0,
        })
        .await,
        api.fetch_input(&claimed(), Scratch::new(JOB_ID).source())
            .await
            .map(drop),
        post(&api).await,
    ];

    // Assert
    for result in results {
        assert!(matches!(result, Err(ApiError::Transient(_))), "{result:?}");
    }
}

#[tokio::test]
async fn a_403_stops_every_call_with_forbidden() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(403)).await;
    let api = client(&peer.base_url());

    // Act
    let results = [
        api.claim(Lane::Image).await.map(drop),
        api.heartbeat(JOB_ID).await.map(drop),
        api.release(JOB_ID).await,
        api.complete(JOB_ID, SkipOutcome::Obsolete).await,
        api.fail(JOB_ID, &failure()).await.map(drop),
        api.report_status(&WorkerStatus {
            decoder_ok: false,
            image_age_days: 0,
        })
        .await,
        post(&api).await,
    ];

    // Assert
    for result in results {
        assert!(matches!(result, Err(ApiError::Forbidden)), "{result:?}");
    }
}

#[tokio::test]
async fn a_status_a_route_does_not_document_is_a_protocol_error_not_another_routes_meaning() {
    // Arrange: 404 on claim must not read as an obsolete revision; 409 on claim and status is no lost lease.
    let missing = Peer::start(|_| Reply::status(404)).await;
    let conflict = Peer::start(|_| Reply::status(409)).await;
    let odd = Peer::start(|_| Reply::status(418)).await;

    // Act
    let claim_404 = client(&missing.base_url()).claim(Lane::Image).await;
    let heartbeat_404 = client(&missing.base_url()).heartbeat(JOB_ID).await;
    let claim_409 = client(&conflict.base_url()).claim(Lane::Image).await;
    let status_409 = client(&conflict.base_url())
        .report_status(&WorkerStatus {
            decoder_ok: true,
            image_age_days: 1,
        })
        .await;
    let post_418 = post(&client(&odd.base_url())).await;

    // Assert
    assert!(
        matches!(claim_404, Err(ApiError::Protocol(_))),
        "{claim_404:?}"
    );
    assert!(
        matches!(heartbeat_404, Err(ApiError::Protocol(_))),
        "{heartbeat_404:?}"
    );
    assert!(
        matches!(claim_409, Err(ApiError::Protocol(_))),
        "{claim_409:?}"
    );
    assert!(
        matches!(status_409, Err(ApiError::Protocol(_))),
        "{status_409:?}"
    );
    assert!(
        matches!(post_418, Err(ApiError::Protocol(_))),
        "{post_418:?}"
    );
}

#[tokio::test]
async fn a_success_status_with_the_wrong_shape_is_a_protocol_error() {
    // Arrange
    let garbled = Peer::start(|_| Reply::json(200, r#"{"job_id":"nope"}"#)).await;
    let wrong_code = Peer::start(|_| Reply::status(200)).await;

    // Act
    let claim = client(&garbled.base_url()).claim(Lane::Image).await;
    let release = client(&wrong_code.base_url()).release(JOB_ID).await;

    // Assert
    assert!(matches!(claim, Err(ApiError::Protocol(_))), "{claim:?}");
    assert!(matches!(release, Err(ApiError::Protocol(_))), "{release:?}");
}
