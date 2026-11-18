//! AZ-3660 (E-22/p08), flow F10 steps 1-3 (the API side) through the real `sunnycloud serve` binary on PostgreSQL 18
//! and a blob volume over HTTP. The test is the HTTP caller in the worker supervisor's place, with the configured
//! worker bearer on the internal listener: jobs born in the commit (AC-1), claim, lease and the scheduled reaper
//! (AC-2), the obsolete input (AC-3), the production callers (AC-4), a 1 GiB input equal to the plaintext (AC-72,
//! fetch part) and the worker routes closed to users and to the public listener (AC-79).
//!
//! NOT-RUN here: the Caddy half of AC-79 (every worker route through Caddy with the release Caddyfile answers 404)
//! runs in the Compose test stack of E-31/c05. Durations are set short through config and polled
//! (`_docs/02_document/tests/environment.md`): `job_lease` at its 5 s minimum, the job reaper every second, and
//! for AC-3 `retention` 0 s with the GC loop every second, so "retention expired" is real time, not a test clock.
//!
//! Known failure, kept honest: AC-3's collected case asserts the contract (`worker_job_protocol.md`: 404
//! `revision_obsolete`, then the job closed `obsolete`), but `jobs` cascades on the revision delete, so the GC
//! removes the job with its revision and the input answers 409 `lease_lost` (08 description, AZ-3842 as built).

mod pipeline_support;
mod root_support;
mod serve_support;

use std::time::{Duration, Instant};

use pipeline_support::large::LargeFile;
use pipeline_support::media::{WORKER_TOKEN, jpeg_bytes};
use pipeline_support::tree::uuid_of;
use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{File, Pipeline, entries, entry, now_micros};
use sc_ids::{hash_block, rollup};
use serde_json::json;
use serve_support::http::call;
use uuid::Uuid;

const GIB: usize = 1024 * 1024 * 1024;
/// `job_lease` at its minimum and the reaper every second.
const SHORT_LEASES: [(&str, &str); 2] = [("job_lease", "5s"), ("job_reap_interval", "1s")];
/// Commit-to-claimable budget of an interactive thumbnail job (NFR of AZ-3660, part of AC-10).
const CLAIMABLE_WITHIN: Duration = Duration::from_secs(1);
const BOUND: Duration = Duration::from_secs(30);

/// A JPEG of `size` bytes committed as `name` under `folder`; its node and revision.
fn commit_jpeg(
    pipeline: &Pipeline,
    folder: Uuid,
    name: &str,
    size: usize,
    seed: u64,
) -> (Uuid, Uuid) {
    let device = &pipeline.device;
    let committed = pipeline.commit_as(device, folder, name, &jpeg_bytes(size, seed), None);
    assert_eq!(committed["status"], "committed", "{committed}");
    (
        uuid_of(&committed, "node_id"),
        uuid_of(&committed, "revision_id"),
    )
}

/// Polls the wall clock (the binary's clock too) until it has passed `until`.
async fn wait_past(until: i64) {
    let deadline = Instant::now() + BOUND;
    while now_micros() <= until {
        assert!(Instant::now() < deadline, "the clock did not pass {until}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_a_committed_jpeg_has_its_interactive_job_and_a_text_file_or_a_refused_entry_has_none()
{
    // Arrange: a JPEG, a text file and a JPEG whose entry is refused, staged and committed in one batch.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac1");
    let mut files = vec![
        File::new("photo.jpg", 50_000, 1),
        File::new("notes.txt", 2_000, 2),
        File::new("refused.jpg", 50_000, 3),
    ];
    files[0].bytes = jpeg_bytes(50_000, 1);
    files[2].bytes = jpeg_bytes(50_000, 3);
    for staged in pipeline.stage_all(&files).await {
        assert_eq!(staged.status, 200, "{staged:?}");
    }
    let mut batch = entries(&files[..2], folder);
    batch.push(entry(files[2].key, folder, "bad/name.jpg"));

    // Act
    let committed = pipeline.commit_batch(Uuid::new_v4(), &batch);
    let committed_at = Instant::now();
    let claimed = pipeline.claim_image("w1");
    let claim_took = committed_at.elapsed();

    // Assert
    assert_eq!(committed.status, 200, "{committed:?}");
    let results = committed.json()["entries"].clone();
    assert_eq!(results[2]["code"], "invalid_name", "{results}");
    let (photo, text) = (
        uuid_of(&results[0], "revision_id"),
        uuid_of(&results[1], "revision_id"),
    );
    let photo_jobs = pipeline.jobs_of(photo).await;
    assert_eq!(photo_jobs.len(), 1, "{photo_jobs:?}");
    assert_eq!(
        (photo_jobs[0].0.as_str(), photo_jobs[0].1.priority),
        ("stills", 0)
    );
    assert_eq!(pipeline.jobs_of(text).await, []);
    assert_eq!(
        pipeline.job_count().await,
        1,
        "the refused entry left a job"
    );
    // Read after the claim, so the one image job shows as running.
    let stats = pipeline.media_stats();
    let lanes = [
        "image_queued",
        "image_running",
        "video_queued",
        "video_running",
    ];
    let shown: Vec<_> = lanes.iter().map(|key| stats[key].clone()).collect();
    assert_eq!(shown, [json!(0), json!(1), json!(0), json!(0)], "{stats}");
    assert_eq!(claimed.status, 200, "{claimed:?}");
    assert_eq!(claimed.json()["kind"], "stills");
    assert!(
        claim_took < CLAIMABLE_WITHIN,
        "claimable after {claim_took:?}"
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_an_expired_lease_is_taken_over_with_attempt_plus_one_and_the_reaper_kills_the_third() {
    // Arrange
    let pipeline = Pipeline::start(&SHORT_LEASES).await;
    let folder = pipeline.folder("ac2");
    commit_jpeg(&pipeline, folder, "photo.jpg", 40_000, 1);

    // Act: w1 claims and extends, then goes silent; w2 and w3 take the job over after each expiry; nobody
    // claims after the third.
    let first = pipeline.claim_image("w1").json();
    let job = uuid_of(&first, "job_id");
    let extended = pipeline.extend_lease("w1", job);
    let (_, lease_end) = pipeline.job(job).await.expect("the job");
    wait_past(lease_end).await;
    let second = pipeline.claim_image("w2").json();
    let stale_holder = pipeline.extend_lease("w1", job);
    wait_past(pipeline.job(job).await.expect("the job").1).await;
    let third = pipeline.claim_image("w3").json();
    wait_past(pipeline.job(job).await.expect("the job").1).await;
    let dead = pipeline
        .wait_job_status(job, "dead", Instant::now() + BOUND)
        .await;
    let after_death = pipeline.claim_image("w4");

    // Assert
    assert_eq!(
        (first["attempt"].clone(), first["lease_ms"].clone()),
        (json!(1), json!(5000))
    );
    assert_eq!(extended.status, 200, "{extended:?}");
    assert_eq!(extended.json()["lease_ms"], 5000);
    assert_eq!(
        (uuid_of(&second, "job_id"), second["attempt"].clone()),
        (job, json!(2))
    );
    assert_eq!(
        stale_holder.json()["code"],
        "lease_lost",
        "{stale_holder:?}"
    );
    assert_eq!(
        (uuid_of(&third, "job_id"), third["attempt"].clone()),
        (job, json!(3))
    );
    assert_eq!(
        (dead.attempts, dead.last_error.as_deref()),
        (3, Some("lease_expired"))
    );
    assert_eq!(after_death.status, 204, "{after_death:?}");
    assert_eq!(pipeline.media_stats()["image_dead"], 1);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "KNOWN-FAIL(AZ-4332): jobs are deleted with their collected revision (ON DELETE CASCADE); the contract wins (owner 2026-11-18), un-ignore with the fix"]
async fn ac3_the_input_of_a_collected_revision_is_404_revision_obsolete_and_the_job_closes_obsolete()
 {
    // Arrange: retention 0 s and the GC loop every second, so a deleted file's revision is collected at once.
    let pipeline = Pipeline::start(&[("retention", "0s"), ("gc_interval", "1s")]).await;
    let folder = pipeline.folder("ac3");
    let (node, revision) = commit_jpeg(&pipeline, folder, "photo.jpg", 40_000, 1);
    let job = uuid_of(&pipeline.claim_image("w1").json(), "job_id");
    assert_eq!(pipeline.delete_with_base(node, Some(revision)).status, 204);
    pipeline
        .wait_revision_collected(revision, Instant::now() + BOUND)
        .await;

    // Act
    let input = pipeline.job_input("w1", job);
    let path = format!("/api/v1/worker-jobs/{job}/result");
    let closed = pipeline.worker_call(
        WORKER_TOKEN,
        "w1",
        "PUT",
        &path,
        r#"{"outcome":"obsolete"}"#,
    );

    // Assert
    assert_eq!(
        (input.status, input.code()),
        (404, "revision_obsolete".to_owned())
    );
    assert_eq!(closed.status, 204, "{closed:?}");
    let (row, _) = pipeline.job(job).await.expect("the closed job");
    assert_eq!(
        (row.status.as_str(), row.outcome.as_deref()),
        ("succeeded", Some("obsolete"))
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_the_input_of_a_deleted_file_still_retained_is_served() {
    // Arrange: the default retention keeps the deleted file's revision.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac3");
    let bytes = jpeg_bytes(1_500_017, 2);
    let committed = pipeline.commit_as(&pipeline.device, folder, "kept.jpg", &bytes, None);
    let (node, revision) = (
        uuid_of(&committed, "node_id"),
        uuid_of(&committed, "revision_id"),
    );
    let job = uuid_of(&pipeline.claim_image("w1").json(), "job_id");
    assert_eq!(pipeline.delete_with_base(node, Some(revision)).status, 204);

    // Act
    let input = pipeline.job_input("w1", job);

    // Assert
    assert_eq!(
        input.status,
        200,
        "{}",
        String::from_utf8_lossy(&input.body)
    );
    assert_eq!(input.length, Some(bytes.len() as u64));
    let expected: Vec<_> = bytes.chunks(sc_ids::BLOCK_SIZE).map(hash_block).collect();
    assert_eq!(input.blocks, expected);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "1 GiB: runs in the release CI job `pipelines-large` (owner decision 2026-11-18)"]
async fn ac72_the_worker_input_of_a_1_gib_file_hashes_to_its_plaintext_block_by_block() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac72");
    let file = LargeFile::jpeg("big.jpg", GIB, 7);
    let upload = pipeline.started(folder, &file);
    let every: Vec<usize> = (0..file.blocks()).collect();
    pipeline.put_blocks(upload, &file, &every, 4).await;
    let committed = pipeline.commit_upload(upload, folder, &file);
    assert_eq!(committed.status, 200, "{committed:?}");
    let node = pipeline.node(uuid_of(&committed.json(), "node_id"));
    let job = uuid_of(&pipeline.claim_image("w1").json(), "job_id");

    // Act
    let input = pipeline.job_input("w1", job);

    // Assert
    assert_eq!(
        input.status,
        200,
        "{}",
        String::from_utf8_lossy(&input.body)
    );
    assert_eq!(input.length, Some(GIB as u64));
    let blocklist: Vec<String> = input.blocks.iter().map(ToString::to_string).collect();
    assert!(
        blocklist == file.blocklist(),
        "a block differs from the plaintext"
    );
    let file_hash = input.file_hash.expect("a file hash").to_string();
    assert_eq!(Some(file_hash.as_str()), node["content_hash"].as_str());
    assert_eq!(rollup(&input.blocks).to_string(), file_hash);
    pipeline.finish().await;
}

/// Every worker operation with a body it accepts.
fn worker_routes(job: Uuid) -> Vec<(&'static str, String, &'static str)> {
    let base = "/api/v1/worker-jobs";
    vec![
        ("POST", format!("{base}/claims"), r#"{"lane":"image"}"#),
        ("PUT", format!("{base}/{job}/lease"), ""),
        ("DELETE", format!("{base}/{job}/lease"), ""),
        ("GET", format!("{base}/{job}/input"), ""),
        ("PUT", format!("{base}/{job}/outputs/thumb"), "x"),
        (
            "PUT",
            format!("{base}/{job}/result"),
            r#"{"outcome":"obsolete"}"#,
        ),
        (
            "POST",
            format!("{base}/{job}/failure"),
            r#"{"code":"x","disposition":"retry"}"#,
        ),
        (
            "PUT",
            format!("{base}/status"),
            r#"{"decoder_ok":true,"image_age_days":1}"#,
        ),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn ac79_worker_routes_answer_403_to_a_session_and_a_device_and_404_on_the_public_listener() {
    // Arrange: one queued job, so a leaked route would show on its row.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac79");
    let (_, revision) = commit_jpeg(&pipeline, folder, "photo.jpg", 40_000, 1);
    let jobs = pipeline.jobs_of(revision).await;
    let job = pipeline.job_ids_of(revision).await[0];
    let users = [pipeline.account.token.clone(), pipeline.device.clone()];

    // Act
    let mut internal = Vec::new();
    let mut public = Vec::new();
    for (method, path, body) in worker_routes(job) {
        for token in &users {
            let answer = pipeline.worker_call(token, "w1", method, &path, body);
            internal.push((
                method,
                path.clone(),
                answer.status,
                answer.json()["code"].clone(),
            ));
        }
        for token in users.iter().chain([&WORKER_TOKEN.to_owned()]) {
            let headers = [("X-Worker-Id", "w1".to_owned())];
            let answer = call(
                pipeline.stack.ports.public,
                method,
                &path,
                token,
                &headers,
                body.as_bytes(),
            );
            public.push((method, path.clone(), answer.status));
        }
    }
    let status = pipeline.worker_call(
        WORKER_TOKEN,
        "w1",
        "PUT",
        "/api/v1/worker-jobs/status",
        r#"{"decoder_ok":true,"image_age_days":1}"#,
    );

    // Assert
    let leaked: Vec<_> = internal
        .iter()
        .filter(|(_, _, status, code)| (*status, code.clone()) != (403, json!("forbidden")))
        .collect();
    assert!(leaked.is_empty(), "{leaked:#?}");
    let public_leaks: Vec<_> = public
        .iter()
        .filter(|(_, _, status)| *status != 404)
        .collect();
    assert!(public_leaks.is_empty(), "{public_leaks:#?}");
    assert_eq!(
        pipeline.jobs_of(revision).await,
        jobs,
        "a refused call changed the job"
    );
    assert_eq!(
        status.status, 204,
        "the worker bearer is refused: {status:?}"
    );
    pipeline.finish().await;
}

/// The F10 joins of stage 1: each operation, the non-test file that calls, binds or registers it, and its text.
const CALLERS: &[Caller] = &[
    (
        "MediaEnqueueEffect in the commit effects list built by 12",
        "server/src/root/lists.rs",
        &[
            "MediaEnqueueEffect::new(planning.queue.clone(), planning.planner)",
            "Arc::new(JobReaper::new(",
            "media.job_reap_interval",
        ],
    ),
    (
        "the commit effects handed to 03",
        "server/src/root/domain.rs",
        &["commit_effects: items(&effects.0)"],
    ),
    (
        "WorkerAuthenticator::authenticate in front of every worker route",
        "components/11_http_api/src/auth/layer.rs",
        &["state.worker_auth.authenticate(presented)"],
    ),
    (
        "WorkerJobs::{claim, extend_lease, release, complete_skipped, fail, open_input, record_status}",
        "components/11_http_api/src/registry/worker/handlers.rs",
        &[
            ".claim(lane, &caller.worker_id)",
            ".extend_lease(job.job_id, &caller.worker_id)",
            ".release(job.job_id, &caller.worker_id)",
            ".complete_skipped(job.job_id, &caller.worker_id, outcome)",
            ".fail(job.job_id, &caller.worker_id, code, disposition)",
            ".open_input(job.job_id, &caller.worker_id)",
            ".record_status(&caller.worker_id, status)",
        ],
    ),
    (
        "the worker routes on the internal listener only",
        "components/11_http_api/src/registry/worker/mod.rs",
        &["RouteSpec::new(method, path, owner, AuthClass::Worker, body).internal()"],
    ),
    (
        "the worker ports bound in the composition root",
        "server/src/root/api.rs",
        &[
            "worker_auth: media.worker_auth.clone()",
            "worker_jobs: media.worker_jobs.clone()",
        ],
    ),
    (
        "the internal listener bound to the configured backend address",
        "server/src/serve/http.rs",
        &["internal_addr: settings.internal_addr.get()"],
    ),
];

#[test]
fn ac4_every_stage_operation_has_a_production_caller_and_the_internal_listener_its_own_address() {
    // Arrange
    let callers = CALLERS;

    // Act
    let missing = missing_callers(callers);

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
}
