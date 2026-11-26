//! AZ-3847 (E-18/c12), harness H1, wire form part 1: the worker operations of `WorkerJobs` over HTTP on the
//! internal listener of the real `sunnycloud serve` binary (`--features test-clock`), on PostgreSQL 18, a blob volume
//! and the real 02, 03, 04 and 08. The test is the worker client holding `worker_api_token`. Cases: IT-08-4 (claim
//! order and lanes), IT-08-6 (lease expiry, `lease_lost` for every call of the old holder), IT-08-8 (release and
//! `result`), IT-08-16 (the uniform 403 of AC-2) and IT-08-20 (status stamped by the API clock).
//!
//! The port forms of the same cases run against the 08 crate (`components/08_media_jobs/tests/media`). Durations:
//! `job_lease` at its 5 s minimum (H1's 2 s is below what the settings accept), and lease expiry by the binary's
//! test clock instead of a wait.
#![cfg(feature = "test-clock")]

mod pipeline_support;
mod root_support;
mod serve_support;

use std::collections::BTreeSet;

use pipeline_support::media::{WEBP, WORKER_TOKEN, jpeg_bytes, mp4_bytes};
use pipeline_support::{File, Pipeline, entry, now_micros, uuid_of};
use serde_json::{Value, json};
use serve_support::http::{Response, call, send};
use sqlx::Connection;
use uuid::Uuid;

const LEASE: (&str, &str) = ("job_lease", "5s");
const LEASE_MS: u64 = 5_000;
const CLAIMS: &str = "/api/v1/worker-jobs/claims";
const STATUS: &str = "/api/v1/worker-jobs/status";
const STATUS_BODY: &str = r#"{"decoder_ok":true,"image_age_days":3}"#;

/// Stages and commits `bytes` as `name` under `folder` in a batch of its own; the new revision.
async fn commit(
    pipeline: &Pipeline,
    folder: Uuid,
    name: &str,
    bytes: Vec<u8>,
    import_hint: bool,
) -> Uuid {
    let mut file = File::new(name, bytes.len(), 0);
    file.bytes = bytes;
    for staged in pipeline.stage_all(std::slice::from_ref(&file)).await {
        assert_eq!(staged.status, 200, "{staged:?}");
    }
    let mut one = entry(file.key, folder, name);
    one["import_hint"] = json!(import_hint);
    let committed = pipeline.commit_batch(Uuid::new_v4(), &[one]);
    assert_eq!(committed.status, 200, "{committed:?}");
    let result = committed.json()["entries"][0].clone();
    assert_eq!(result["status"], "committed", "{result}");
    uuid_of(&result, "revision_id")
}

/// The one job of `revision`.
async fn only_job(pipeline: &Pipeline, revision: Uuid) -> Uuid {
    let jobs = pipeline.job_ids_of(revision).await;
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    jobs[0]
}

/// `(job_id, kind, attempt, lease_ms)` of a 200 claim.
fn claimed(response: &Response) -> (Uuid, String, u64, u64) {
    assert_eq!(response.status, 200, "{response:?}");
    let body = response.json();
    let number = |member: &str| body[member].as_u64().expect("a number");
    let kind = body["kind"].as_str().expect("a kind").to_owned();
    (
        uuid_of(&body, "job_id"),
        kind,
        number("attempt"),
        number("lease_ms"),
    )
}

fn assert_nothing_due(response: &Response) {
    assert_eq!(
        (response.status, response.body.len()),
        (204, 0),
        "{response:?}"
    );
}

fn assert_lease_lost(response: &Response) {
    assert_eq!(
        (response.status, response.json()["code"].clone()),
        (409, json!("lease_lost")),
        "{response:?}"
    );
}

/// `(status, attempts, leased_by)` of `job`.
async fn lease_row(pipeline: &Pipeline, job: Uuid) -> (String, i16, Option<String>) {
    let mut admin = pipeline.stack.root.db.admin().await;
    let row = sqlx::query_as("SELECT status, attempts, leased_by FROM jobs WHERE id = $1")
        .bind(job)
        .fetch_one(&mut admin)
        .await
        .expect("the job row");
    admin.close().await.expect("close admin connection");
    row
}

fn job_path(job: Uuid, rest: &str) -> String {
    format!("/api/v1/worker-jobs/{job}/{rest}")
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_4_claims_over_http_take_priority_then_age_and_never_cross_lanes() {
    // Arrange: three bulk photos (import hint), each in its own commit, then one interactive photo and a clip.
    let pipeline = Pipeline::start(&[LEASE]).await;
    let folder = pipeline.folder("it4");
    let mut bulk = Vec::new();
    for seed in 1..=3 {
        let bytes = jpeg_bytes(30_000, seed);
        let revision = commit(&pipeline, folder, &format!("bulk{seed}.jpg"), bytes, true).await;
        bulk.push(only_job(&pipeline, revision).await);
    }
    let photo = commit(&pipeline, folder, "now.jpg", jpeg_bytes(30_000, 9), false).await;
    let interactive = only_job(&pipeline, photo).await;
    let clip = commit(&pipeline, folder, "clip.mp4", mp4_bytes(40_000, 10), false).await;

    // Act
    let image: Vec<Response> = (0..5).map(|_| pipeline.claim("w1", "image")).collect();
    let video: Vec<Response> = (0..3).map(|_| pipeline.claim("w2", "video")).collect();

    // Assert
    for job in &bulk {
        let (row, _) = pipeline.job(*job).await.expect("a bulk job");
        assert_eq!(row.priority, 1, "an import-hint job is bulk");
    }
    let order: Vec<Uuid> = image[..4].iter().map(|answer| claimed(answer).0).collect();
    let mut want = vec![interactive];
    want.extend(&bulk);
    assert_eq!(
        order, want,
        "the interactive job first, then the bulk jobs by age"
    );
    assert_nothing_due(&image[4]);
    let kinds: BTreeSet<String> = video[..2].iter().map(|answer| claimed(answer).1).collect();
    let ids: BTreeSet<Uuid> = video[..2].iter().map(|answer| claimed(answer).0).collect();
    assert_eq!(kinds, BTreeSet::from(["poster".into(), "transcode".into()]));
    assert_eq!(ids, pipeline.job_ids_of(clip).await.into_iter().collect());
    assert_nothing_due(&video[2]);
    for (answer, worker) in image[..4]
        .iter()
        .zip(["w1"; 4])
        .chain(video[..2].iter().zip(["w2"; 2]))
    {
        let (job, _, attempt, lease_ms) = claimed(answer);
        assert_eq!((attempt, lease_ms), (1, LEASE_MS), "{answer:?}");
        let (status, attempts, leased_by) = lease_row(&pipeline, job).await;
        assert_eq!(
            (status.as_str(), attempts, leased_by.as_deref()),
            ("running", 1, Some(worker))
        );
    }
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_6_after_the_lease_expires_every_call_of_the_old_holder_is_409_lease_lost() {
    // Arrange: w1 holds the job, then the API clock passes the lease.
    let pipeline = Pipeline::start(&[LEASE]).await;
    let folder = pipeline.folder("it6");
    commit(&pipeline, folder, "photo.jpg", jpeg_bytes(30_000, 1), false).await;
    let (job, ..) = claimed(&pipeline.claim("w1", "image"));
    pipeline.advance_clock(6);

    // Act
    let taken = pipeline.claim("w2", "image");
    let failure = r#"{"code":"decode_failed","disposition":"retry"}"#;
    let skipped = r#"{"outcome":"skipped_compliant"}"#;
    let stale = [
        pipeline.extend_lease("w1", job),
        pipeline.put_output_as("w1", job, "thumb", "image/webp", &WEBP, Some((1, 1))),
        pipeline.worker_call(
            WORKER_TOKEN,
            "w1",
            "POST",
            &job_path(job, "failure"),
            failure,
        ),
        pipeline.worker_call(WORKER_TOKEN, "w1", "PUT", &job_path(job, "result"), skipped),
        pipeline.worker_call(WORKER_TOKEN, "w1", "DELETE", &job_path(job, "lease"), ""),
    ];
    let stranger = pipeline.extend_lease("w3", job);
    let holder = pipeline.extend_lease("w2", job);

    // Assert
    let (again, _, attempt, _) = claimed(&taken);
    assert_eq!((again, attempt), (job, 2));
    for answer in stale.iter().chain([&stranger]) {
        assert_lease_lost(answer);
        let body = answer.json();
        for member in ["job_id", "kind", "content_hash", "lease_ms", "attempt"] {
            assert!(body.get(member).is_none(), "job data in {body}");
        }
    }
    assert_eq!(
        (holder.status, holder.json()["lease_ms"].as_u64()),
        (200, Some(LEASE_MS)),
        "{holder:?}"
    );
    let (status, attempts, leased_by) = lease_row(&pipeline, job).await;
    assert_eq!(
        (status.as_str(), attempts, leased_by.as_deref()),
        ("running", 2, Some("w2"))
    );
    assert_eq!(pipeline.derived().await, []);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_8_a_release_gives_the_attempt_back_and_a_result_closes_the_job_without_output() {
    // Arrange: three photos, all claimed by w1 (oldest first).
    let pipeline = Pipeline::start(&[LEASE]).await;
    let folder = pipeline.folder("it8");
    let mut jobs = Vec::new();
    for seed in 1..=3 {
        let bytes = jpeg_bytes(30_000, seed);
        let revision = commit(&pipeline, folder, &format!("p{seed}.jpg"), bytes, false).await;
        jobs.push(only_job(&pipeline, revision).await);
    }
    let held: Vec<Uuid> = (0..3)
        .map(|_| claimed(&pipeline.claim("w1", "image")).0)
        .collect();
    assert_eq!(held, jobs);
    let (released_job, skipped_job, obsolete_job) = (jobs[0], jobs[1], jobs[2]);
    let skipped = r#"{"outcome":"skipped_compliant"}"#;
    let obsolete = r#"{"outcome":"obsolete"}"#;

    // Act
    let lease = job_path(released_job, "lease");
    let stranger_release = pipeline.worker_call(WORKER_TOKEN, "w9", "DELETE", &lease, "");
    let stranger_result = pipeline.worker_call(
        WORKER_TOKEN,
        "w9",
        "PUT",
        &job_path(skipped_job, "result"),
        skipped,
    );
    let released = pipeline.worker_call(WORKER_TOKEN, "w1", "DELETE", &lease, "");
    let after_release = lease_row(&pipeline, released_job).await;
    let reclaimed = pipeline.claim("w2", "image");
    let closed_skipped = pipeline.worker_call(
        WORKER_TOKEN,
        "w1",
        "PUT",
        &job_path(skipped_job, "result"),
        skipped,
    );
    let closed_obsolete = pipeline.worker_call(
        WORKER_TOKEN,
        "w1",
        "PUT",
        &job_path(obsolete_job, "result"),
        obsolete,
    );

    // Assert
    assert_lease_lost(&stranger_release);
    assert_lease_lost(&stranger_result);
    assert_eq!(
        (released.status, released.body.len()),
        (204, 0),
        "{released:?}"
    );
    assert_eq!(after_release, ("queued".to_owned(), 0, None));
    let (again, _, attempt, _) = claimed(&reclaimed);
    assert_eq!((again, attempt), (released_job, 1));
    for (answer, job, outcome) in [
        (&closed_skipped, skipped_job, "skipped_compliant"),
        (&closed_obsolete, obsolete_job, "obsolete"),
    ] {
        assert_eq!(answer.status, 204, "{answer:?}");
        let (row, _) = pipeline.job(job).await.expect("the closed job");
        assert_eq!(
            (row.status.as_str(), row.outcome.as_deref()),
            ("succeeded", Some(outcome))
        );
    }
    assert_eq!(pipeline.derived().await, [], "a result posts no output");
    pipeline.finish().await;
}

/// Status, headers and body of a refusal, without what differs per request: the `X-Request-Id` and `Date`
/// headers and the body's `request_id`, which repeats `X-Request-Id`.
fn comparable(answer: &Response) -> (u16, Vec<(String, String)>, Value) {
    let headers = answer
        .headers
        .iter()
        .filter(|(name, _)| name != "x-request-id" && name != "date")
        .cloned()
        .collect();
    let mut body = answer.json();
    let request_id = body
        .as_object_mut()
        .and_then(|members| members.remove("request_id"));
    assert_eq!(
        request_id.as_ref().and_then(Value::as_str),
        answer.header("x-request-id"),
        "the body's request_id is the header's"
    );
    (answer.status, headers, body)
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_16_six_refused_credentials_get_one_identical_403_and_a_worker_on_a_job_it_does_not_hold_gets_409()
 {
    // Arrange: a job held by w1, and six credentials that are not the worker secret.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("it16");
    commit(&pipeline, folder, "photo.jpg", jpeg_bytes(30_000, 1), false).await;
    let (job, ..) = claimed(&pipeline.claim("w1", "image"));
    let wrong = "sc_wrk_not-the-worker-secret";
    let session = pipeline.account.token.clone();
    let credentials: [(&str, Option<(&str, String)>); 6] = [
        ("no header", None),
        ("empty bearer", Some(("Authorization", "Bearer ".into()))),
        (
            "wrong secret",
            Some(("Authorization", format!("Bearer {wrong}"))),
        ),
        (
            "user session",
            Some(("Authorization", format!("Bearer {session}"))),
        ),
        (
            "device token",
            Some(("Authorization", format!("Bearer {}", pipeline.device))),
        ),
        (
            "session cookie",
            Some(("Cookie", format!("__Host-session={session}"))),
        ),
    ];
    let input = job_path(job, "input");
    let routes = [
        ("POST", CLAIMS, r#"{"lane":"image"}"#),
        ("GET", input.as_str(), ""),
        ("PUT", STATUS, STATUS_BODY),
    ];

    // Act
    let mut answers = Vec::new();
    for (method, path, body) in routes {
        for (name, credential) in &credentials {
            let mut headers = vec![("X-Worker-Id", "w1".to_owned())];
            headers.extend(credential.clone());
            if !body.is_empty() {
                headers.push(("Content-Type", "application/json".to_owned()));
            }
            let addr = pipeline.stack.ports.internal;
            let answer = send(addr, method, path, &headers, body.as_bytes());
            answers.push((format!("{name} on {method} {path}"), answer));
        }
    }
    let not_holder = pipeline.worker_call(WORKER_TOKEN, "w2", "GET", &input, "");

    // Assert
    let first = comparable(&answers[0].1);
    assert_eq!(first.0, 403, "{:?}", answers[0]);
    for (name, answer) in &answers {
        assert_eq!(comparable(answer), first, "{name}");
    }
    assert_lease_lost(&not_holder);
    let log = pipeline.server.lines().join("\n");
    for secret in [wrong, session.as_str(), pipeline.device.as_str()] {
        assert!(
            !log.contains(secret),
            "a refused credential reached the log"
        );
    }
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_20_worker_status_is_stamped_by_the_api_clock_whatever_the_client_says() {
    // Arrange: the API clock one hour ahead of the wall clock; the request claims a date in 2015.
    let pipeline = Pipeline::start(&[]).await;
    let offset = i64::try_from(pipeline.advance_clock(3_600)).expect("an offset") * 1_000_000;
    let headers = [
        ("X-Worker-Id", "w-status".to_owned()),
        ("Content-Type", "application/json".to_owned()),
        ("Date", "Thu, 01 Jan 2015 00:00:00 GMT".to_owned()),
    ];

    // Act
    let before = now_micros() + offset;
    let addr = pipeline.stack.ports.internal;
    let reported = call(
        addr,
        "PUT",
        STATUS,
        WORKER_TOKEN,
        &headers,
        STATUS_BODY.as_bytes(),
    );
    let after = now_micros() + offset;

    // Assert
    assert_eq!(
        (reported.status, reported.body.len()),
        (204, 0),
        "{reported:?}"
    );
    let mut admin = pipeline.stack.root.db.admin().await;
    let (seen_at, age, decoder_ok): (i64, i32, bool) = sqlx::query_as(
        "SELECT (extract(epoch FROM seen_at) * 1000000)::bigint, image_age_days, decoder_ok \
         FROM worker_status WHERE worker_id = 'w-status'",
    )
    .fetch_one(&mut admin)
    .await
    .expect("the status row");
    admin.close().await.expect("close admin connection");
    assert!(
        (before..=after).contains(&seen_at),
        "seen_at {seen_at} outside the API clock's [{before}, {after}]"
    );
    assert_eq!((age, decoder_ok), (3, true));
    pipeline.finish().await;
}
