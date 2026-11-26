//! AZ-3847 (E-18/c12), harness H1, wire form part 2: outputs and input of `WorkerJobs` over HTTP on the internal
//! listener of the real `sunnycloud serve` binary (`--features test-clock`), on PostgreSQL 18, a blob volume and the
//! real 02, 03, 04 and 08. Files are uploaded through 03 (stage and commit). Cases: IT-08-9 (stored encrypted,
//! registered, job closed after the last output), IT-08-10 (a repeat or a race stores one output, the surplus blob
//! is collected), IT-08-11 (refusals change nothing), IT-08-12 (the input is the whole plaintext) and the collected
//! revision's 404 on input and output.
//!
//! IT-08-11's "lying `Content-Length` smaller than the stream" has no wire form: HTTP/1.1 frames the body by
//! `Content-Length`, so the extra bytes are a next request, not part of this output. The port form refuses a stream
//! longer than its declared size (`08_media_jobs/tests/media/accept_refuse.rs`).
#![cfg(feature = "test-clock")]

mod pipeline_support;
mod root_support;
mod serve_support;

use std::fs;
use std::time::{Duration, Instant};

use pipeline_support::media::{
    WEBP, WORKER_TOKEN, jpeg_bytes, mp4_bytes, problem_code, volume_files,
};
use pipeline_support::{Pipeline, gc::Stored, uuid_of};
use sc_ids::{hash_block, rollup};
use serde_json::json;
use serve_support::http::call;
use uuid::Uuid;

const BOUND: Duration = Duration::from_secs(30);
const LEASE: (&str, &str) = ("job_lease", "5s");

#[tokio::test(flavor = "multi_thread")]
async fn it_08_9_valid_outputs_are_stored_encrypted_and_registered_and_close_the_job_after_the_last_one()
 {
    // Arrange: a photo and a clip uploaded through 03; their stills and transcode jobs claimed.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("it9");
    pipeline.commit_file(folder, "photo.jpg", &jpeg_bytes(40_000, 1));
    pipeline.commit_file(folder, "clip.mp4", &mp4_bytes(60_000, 2));
    let used = "SELECT used_bytes FROM quota WHERE account_id = $1";
    let used_before = pipeline.account_number(used).await;
    let stills = pipeline.claim_kind("w1", "image", "stills");
    let transcode = pipeline.claim_kind("w1", "video", "transcode");

    // Act
    let preview = pipeline.put_output_as(
        "w1",
        stills,
        "preview",
        "image/webp",
        &WEBP,
        Some((1440, 1080)),
    );
    let (between, _) = pipeline.job(stills).await.expect("the stills job");
    let thumb =
        pipeline.put_output_as("w1", stills, "thumb", "image/webp", &WEBP, Some((320, 240)));
    let clip = mp4_bytes(50_000, 3);
    let video = pipeline.put_output_as(
        "w1",
        transcode,
        "transcode",
        "video/mp4",
        &clip,
        Some((1280, 720)),
    );

    // Assert
    for answer in [&preview, &thumb, &video] {
        assert_eq!((answer.status, answer.body.len()), (204, 0), "{answer:?}");
    }
    assert_eq!(
        between.status, "running",
        "the job waits for its last output"
    );
    for job in [stills, transcode] {
        let (row, _) = pipeline.job(job).await.expect("the job");
        assert_eq!(
            (row.status.as_str(), row.outcome.as_deref()),
            ("succeeded", Some("derived"))
        );
    }
    let rows = pipeline.derived_rows().await;
    let shown: Vec<_> = rows
        .iter()
        .map(|(kind, media_type, width, height, _)| {
            (kind.as_str(), media_type.as_str(), *width, *height)
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("preview", "image/webp", Some(1440), Some(1080)),
            ("thumb", "image/webp", Some(320), Some(240)),
            ("transcode", "video/mp4", Some(1280), Some(720)),
        ]
    );
    for (kind, .., blob) in &rows {
        let stored = fs::read(pipeline.blob_file(*blob)).expect("the derived blob file");
        let webp = stored.len() >= 12 && &stored[..4] == b"RIFF" && &stored[8..12] == b"WEBP";
        let mp4 = stored.len() >= 8 && &stored[4..8] == b"ftyp";
        assert!(!webp && !mp4, "the {kind} blob is stored in the clear");
    }
    let committed = "SELECT count(*) FROM blobs b JOIN derived d ON d.blob_id = b.id \
                     WHERE b.account_id = $1 AND b.state = 'committed'";
    assert_eq!(pipeline.account_number(committed).await, 3);
    assert_eq!(
        pipeline.account_number(used).await,
        used_before,
        "derived files are not quota"
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_10_a_repeated_or_raced_output_is_stored_once_and_the_surplus_blobs_are_collected() {
    // Arrange: three photos with their stills jobs claimed; retry backoff 200 ms. GC keeps its 5 min default
    // until the surplus is counted, then the binary restarts with GC every second.
    let mut pipeline = Pipeline::start(&[("job_retry_backoff", "200ms")]).await;
    let folder = pipeline.folder("it10");
    for seed in 1..=3 {
        pipeline.commit_file(folder, &format!("p{seed}.jpg"), &jpeg_bytes(30_000, seed));
    }
    let [repeated, raced, retried] = [0; 3].map(|_| pipeline.claim_kind("w1", "image", "stills"));
    let put = |job: Uuid, kind: &str| {
        pipeline.put_output_as("w1", job, kind, "image/webp", &WEBP, Some((1, 1)))
    };

    // Act
    let (first, second) = (put(repeated, "thumb"), put(repeated, "thumb"));
    let addr = pipeline.stack.ports.internal;
    let path = format!("/api/v1/worker-jobs/{raced}/outputs/thumb");
    let race: Vec<u16> = std::thread::scope(|scope| {
        let posts: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let headers = [
                        ("Content-Type", "image/webp".to_owned()),
                        ("X-Worker-Id", "w1".to_owned()),
                    ];
                    call(addr, "PUT", &path, WORKER_TOKEN, &headers, &WEBP).status
                })
            })
            .collect();
        posts
            .into_iter()
            .map(|post| post.join().expect("a post"))
            .collect()
    });
    let stored_preview = put(retried, "preview");
    let failure = r#"{"code":"decode_failed","disposition":"retry"}"#;
    let failure_path = format!("/api/v1/worker-jobs/{retried}/failure");
    let requeued = pipeline.worker_call(WORKER_TOKEN, "w1", "POST", &failure_path, failure);
    let deadline = Instant::now() + BOUND;
    let again = loop {
        let answer = pipeline.claim("w2", "image");
        if answer.status == 200 {
            break answer.json();
        }
        assert!(Instant::now() < deadline, "the retried job never came back");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let retry_preview = pipeline.put_output_as("w2", retried, "preview", "image/webp", &WEBP, None);
    let retry_thumb = pipeline.put_output_as("w2", retried, "thumb", "image/webp", &WEBP, None);
    let surplus = pipeline.surplus_blobs().await;

    // Assert
    assert_eq!((first.status, second.status), (204, 200));
    let accepted = race.iter().filter(|status| **status == 204).count();
    let present = race.iter().filter(|status| **status == 200).count();
    assert_eq!((accepted, present), (1, 7), "{race:?}");
    assert_eq!(stored_preview.status, 204, "{stored_preview:?}");
    assert_eq!(requeued.json()["result"], "requeued", "{requeued:?}");
    assert_eq!(
        (uuid_of(&again, "job_id"), again["attempt"].clone()),
        (retried, json!(2))
    );
    assert_eq!((retry_preview.status, retry_thumb.status), (200, 204));
    let (row, _) = pipeline.job(retried).await.expect("the retried job");
    assert_eq!(row.status, "succeeded");
    let kinds: Vec<String> = pipeline
        .derived_rows()
        .await
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(
        kinds,
        ["preview", "thumb", "thumb", "thumb"],
        "one row per content and kind"
    );
    assert!(
        surplus.len() >= 8,
        "the repeat and the race left {} surplus blobs",
        surplus.len()
    );
    let due = "SELECT count(*) FROM blob_gc WHERE account_id = $1 AND reason = 'derived_removed' \
               AND due_at <= now() + interval '1 second'";
    assert_eq!(
        usize::try_from(pipeline.account_number(due).await).expect("a count"),
        surplus.len(),
        "every surplus blob is queued for GC, due now"
    );
    for blob in &surplus {
        let file = pipeline.blob_file(*blob).exists();
        assert!(file, "the surplus blob {blob} has its file until GC runs");
    }
    pipeline.stop();
    pipeline.stack.root.setup.set("gc_interval", "1s");
    pipeline.restart();
    pipeline
        .wait_stored(&surplus, Stored::GONE, Instant::now() + BOUND)
        .await;
    assert_eq!(pipeline.surplus_blobs().await, Vec::<Uuid>::new());
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_11_refused_outputs_leave_the_job_the_rows_and_the_volume_as_they_were() {
    // Arrange: a stills job and a transcode job held by w1; the thumbnail cap at 64 KiB.
    let pipeline = Pipeline::start(&[LEASE, ("derived_max_thumb", "64KiB")]).await;
    let folder = pipeline.folder("it11");
    pipeline.commit_file(folder, "photo.jpg", &jpeg_bytes(30_000, 1));
    pipeline.commit_file(folder, "clip.mp4", &mp4_bytes(40_000, 2));
    let stills = pipeline.claim_kind("w1", "image", "stills");
    let transcode = pipeline.claim_kind("w1", "video", "transcode");
    let volume = pipeline.stack.blob_root().join("blobs");
    let files_before = volume_files(&volume);
    let blobs = "SELECT count(*) FROM blobs WHERE account_id = $1 AND state = 'committed'";
    let blobs_before = pipeline.account_number(blobs).await;
    let mut too_big = WEBP.to_vec();
    too_big.resize(65 * 1024, 0);
    let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00";
    let raw = |kind: &str| {
        let headers = [
            ("Content-Type", "image/webp".to_owned()),
            ("X-Worker-Id", "w1".to_owned()),
        ];
        let path = format!("/api/v1/worker-jobs/{stills}/outputs/{kind}");
        call(
            pipeline.stack.ports.internal,
            "PUT",
            &path,
            WORKER_TOKEN,
            &headers,
            &WEBP,
        )
    };

    // Act
    let over_cap =
        pipeline.put_output_as("w1", stills, "thumb", "image/webp", &too_big, Some((1, 1)));
    let png_thumb = pipeline.put_output_as("w1", stills, "thumb", "image/webp", png, Some((1, 1)));
    let text_clip = pipeline.put_output_as(
        "w1",
        transcode,
        "transcode",
        "video/mp4",
        b"plain text, not a clip",
        None,
    );
    let mp4_thumb = pipeline.put_output_as(
        "w1",
        stills,
        "thumb",
        "image/webp",
        &mp4_bytes(2_000, 3),
        None,
    );
    let wrong_kind = pipeline.put_output_as(
        "w1",
        stills,
        "transcode",
        "video/mp4",
        &mp4_bytes(2_000, 4),
        None,
    );
    let traversal = raw("..%2Fx");
    let unknown = raw("unknown");
    pipeline.advance_clock(6);
    let expired = pipeline.put_output_as("w1", stills, "thumb", "image/webp", &WEBP, Some((1, 1)));

    // Assert
    // The HTTP layer refuses a declared length over the kind's cap before 08 reads a byte (`size_too_large`);
    // 08's own `output_too_large` is for a stream longer than its declared length. The contract leaves the two
    // names open (worker_job_protocol.md open point 5).
    assert_eq!(
        problem_code(&over_cap),
        (413, "size_too_large".into()),
        "{over_cap:?}"
    );
    for answer in [&png_thumb, &text_clip, &mp4_thumb] {
        assert_eq!(
            problem_code(answer),
            (422, "output_type_rejected".into()),
            "{answer:?}"
        );
    }
    assert_eq!(
        problem_code(&wrong_kind),
        (422, "output_kind_rejected".into()),
        "{wrong_kind:?}"
    );
    for answer in [&traversal, &unknown] {
        assert!([404, 422].contains(&answer.status), "{answer:?}");
    }
    assert_eq!(
        problem_code(&expired),
        (409, "lease_lost".into()),
        "{expired:?}"
    );
    for job in [stills, transcode] {
        let (row, _) = pipeline.job(job).await.expect("the job");
        assert_eq!((row.status.as_str(), row.attempts), ("running", 1), "{job}");
    }
    assert_eq!(pipeline.derived_rows().await, []);
    assert_eq!(
        volume_files(&volume),
        files_before,
        "a refused output left a file"
    );
    assert_eq!(
        pipeline.account_number(blobs).await,
        blobs_before,
        "a refused output left a committed blob"
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_12_the_input_is_the_whole_plaintext_not_stored_ignores_range_and_outlives_an_overwrite()
 {
    // Arrange: a photo uploaded through 03, its job claimed, then the file overwritten (the old revision is retained).
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("it12");
    let bytes = jpeg_bytes(300_000, 7);
    let (_, revision) = pipeline.commit_file(folder, "photo.jpg", &bytes);
    let claimed = pipeline.claim("w1", "image").json();
    let job = uuid_of(&claimed, "job_id");
    let newer = pipeline.commit_as(
        &pipeline.device,
        folder,
        "photo.jpg",
        &jpeg_bytes(300_000, 8),
        Some(revision),
    );
    assert_eq!(newer["status"], "committed", "{newer}");
    let path = format!("/api/v1/worker-jobs/{job}/input");
    let get = |headers: &[(&str, String)]| {
        call(
            pipeline.stack.ports.internal,
            "GET",
            &path,
            WORKER_TOKEN,
            headers,
            b"",
        )
    };

    // Act
    let whole = get(&[("X-Worker-Id", "w1".to_owned())]);
    let ranged = get(&[
        ("X-Worker-Id", "w1".to_owned()),
        ("Range", "bytes=0-99".to_owned()),
    ]);
    let streamed = pipeline.job_input("w1", job);
    let other = get(&[("X-Worker-Id", "w2".to_owned())]);

    // Assert
    let length = bytes.len().to_string();
    for answer in [&whole, &ranged] {
        assert_eq!(answer.status, 200, "{answer:?}");
        assert_eq!(answer.header("content-length"), Some(length.as_str()));
        assert_eq!(answer.header("cache-control"), Some("no-store"));
        assert_eq!(answer.header("content-range"), None);
        assert!(
            answer.body == bytes,
            "the input is not the uploaded plaintext"
        );
    }
    let manifest = rollup(&[hash_block(&bytes)]).to_string();
    assert_eq!(claimed["content_hash"], manifest.as_str());
    assert_eq!(
        streamed.file_hash.map(|hash| hash.to_string()),
        Some(manifest)
    );
    assert_eq!(
        problem_code(&other),
        (409, "lease_lost".into()),
        "{other:?}"
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn it_08_12_a_collected_revision_is_404_revision_obsolete_on_input_and_output_and_result_closes_it()
 {
    // Arrange: retention 0 s and GC every second, so the deleted file's revision is collected at once.
    let pipeline = Pipeline::start(&[("retention", "0s"), ("gc_interval", "1s")]).await;
    let folder = pipeline.folder("it12c");
    let (node, revision) = pipeline.commit_file(folder, "photo.jpg", &jpeg_bytes(30_000, 1));
    let job = pipeline.claim_kind("w1", "image", "stills");
    assert_eq!(pipeline.delete_with_base(node, Some(revision)).status, 204);
    pipeline
        .wait_revision_collected(revision, Instant::now() + BOUND)
        .await;

    // Act
    let input = pipeline.job_input("w1", job);
    let output = pipeline.put_output_as("w1", job, "thumb", "image/webp", &WEBP, Some((1, 1)));
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
    assert_eq!(
        problem_code(&output),
        (404, "revision_obsolete".into()),
        "{output:?}"
    );
    assert_eq!(pipeline.derived_rows().await, []);
    assert_eq!(closed.status, 204, "{closed:?}");
    let (row, _) = pipeline.job(job).await.expect("the job row stays");
    assert_eq!(
        (row.status.as_str(), row.outcome.as_deref()),
        ("succeeded", Some("obsolete"))
    );
    pipeline.finish().await;
}
