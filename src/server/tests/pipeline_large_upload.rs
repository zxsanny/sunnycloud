//! AZ-3655 (E-22/p03), flow F4 through the real `sunnycloud serve` binary on PostgreSQL 18 and a blob volume over
//! HTTP: a 64 MiB file over four HTTP/2 lanes (AC-1), resume after a dropped connection (AC-2), typed refusals
//! (AC-3), the account guard and the commit effects inside the commit transaction (AC-4), session expiry from the
//! scheduler (AC-5), the production callers of every F4 operation (AC-6) and a SIGKILL before and during the commit
//! (AC-68).
//!
//! Durations are set short through config (`_docs/02_document/tests/environment.md`; the test clock is only for
//! ACs that assert a production default): `upload_session_ttl` and `orphan_sweep_age` at their minimum (1 min),
//! the reaper and the orphan sweep every few seconds, and the tests poll until the outcome shows.

mod pipeline_support;
mod root_support;
mod serve_support;

use std::thread;
use std::time::{Duration, Instant};

use pipeline_support::large::{LargeFile, indexes};
use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{Pipeline, WITHIN};
use sc_ids::{BLOCK_SIZE, hash_block};
use serde_json::Value;
use serve_support::http::{Response, call};
use uuid::Uuid;

const MIB: usize = 1024 * 1024;
const LANES: usize = 4;
/// `upload_session_ttl` at its minimum, the session reaper every second.
const SHORT_SESSIONS: [(&str, &str); 2] = [
    ("upload_session_ttl", "1m"),
    ("session_reap_interval", "1s"),
];
/// Long enough for a session to expire (1 min after its last checkpoint) and the reaper to run.
const EXPIRY_BOUND: Duration = Duration::from_secs(150);

fn all_blocks(file: &LargeFile) -> Vec<usize> {
    (0..file.blocks()).collect()
}

fn id(value: &Value, member: &str) -> Uuid {
    value[member]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no `{member}` in {value}"))
}

/// A long-poll of the account's feed from its head, held on its own connection.
fn hold_long_poll(pipeline: &Pipeline) -> thread::JoinHandle<(Response, Instant)> {
    let path = format!("/api/v1/changes?cursor={}&wait=25", pipeline.feed_cursor());
    let (public, device) = (pipeline.stack.ports.public, pipeline.device.clone());
    thread::spawn(move || {
        let answer = call(public, "GET", &path, &device, &[], &[]);
        (answer, Instant::now())
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_a_64_mib_file_over_four_h2_lanes_commits_wakes_a_long_poll_and_downloads_with_the_same_hash()
 {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac1");
    let file = LargeFile::new("large.bin", 64 * MIB, 3);
    let seq_before = pipeline.feed_seq();
    let poll = hold_long_poll(&pipeline);
    let upload = pipeline.started(folder, &file);
    pipeline
        .put_blocks(upload, &file, &all_blocks(&file), LANES)
        .await;
    let held_until_commit = !poll.is_finished();

    // Act
    let sent = Instant::now();
    let committed = pipeline.commit_upload(upload, folder, &file);
    let (polled, woke_at) = poll.join().expect("the long-poll thread ends");
    let seq_after = pipeline.feed_seq();

    // Assert
    assert_eq!(committed.status, 200, "{committed:?}");
    let committed = committed.json();
    assert_eq!(committed["outcome"], "published");
    let node = id(&committed, "node_id");
    assert_eq!(
        pipeline.node(node)["current_revision"],
        committed["revision_id"]
    );
    assert_eq!(seq_after, seq_before + 1);
    assert!(
        held_until_commit,
        "the long-poll answered before the commit"
    );
    assert_eq!(polled.status, 200, "{polled:?}");
    let woken: Vec<Uuid> = polled.json()["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| id(item, "node_id"))
        .collect();
    assert_eq!(woken, [node]);
    assert!(
        woke_at.duration_since(sent) < WITHIN,
        "the poll was not woken"
    );
    assert_eq!(
        hash_block(&pipeline.download(node)),
        hash_block(&file.bytes)
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_after_a_dropped_connection_the_session_asks_only_for_the_missing_blocks() {
    // Arrange: 10 of 32 blocks acknowledged, block 10 cut off mid-body.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac2");
    let file = LargeFile::new("resumed.bin", 32 * BLOCK_SIZE, 5);
    let upload = pipeline.started(folder, &file);
    for index in 0..10 {
        assert_eq!(pipeline.put_block(upload, &file, index).status, 204);
    }
    pipeline.drop_block_midway(upload, &file, 10);

    // Act
    let state = pipeline.upload_state(upload);
    let need = indexes(&state.json()["need_blocks"]);
    pipeline.put_blocks(upload, &file, &need, LANES).await;
    let committed = pipeline.commit_upload(upload, folder, &file);

    // Assert
    assert_eq!(need, (10..32).collect::<Vec<_>>());
    assert_eq!(committed.status, 200, "{committed:?}");
    let stored = pipeline.download(id(&committed.json(), "node_id"));
    assert!(
        stored
            .chunks(BLOCK_SIZE)
            .all(|block| block.iter().any(|byte| *byte != 0)),
        "a zero-filled block"
    );
    assert!(stored == file.bytes, "the stored file differs");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_quota_bad_hash_early_commit_and_bad_name_reach_the_client_as_typed_refusals() {
    // Arrange: a 16 MiB quota, a 4-block session within it.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac3");
    pipeline
        .arrange("UPDATE quota SET limit_bytes = 16777216 WHERE account_id = $1")
        .await;
    let too_big = LargeFile::new("too-big.bin", 64 * MIB, 6);
    let file = LargeFile::new("partial.bin", 4 * BLOCK_SIZE, 7);
    let upload = pipeline.started(folder, &file);
    let reserved_before = pipeline.reserved_bytes().await;
    let wrong = hash_block(b"some other block").to_string();
    assert_eq!(pipeline.put_block(upload, &file, 1).status, 204);

    // Act
    let over_quota = pipeline.start_upload(folder, &too_big, &too_big.name);
    let reserved_after = pipeline.reserved_bytes().await;
    let bad_hash = pipeline.put_block_hashed(upload, &file, 0, &wrong);
    let state = pipeline.upload_state(upload).json();
    let early = pipeline.commit_upload(upload, folder, &file);
    let bad_name = pipeline.start_upload(folder, &file, "bad/name");

    // Assert
    assert_eq!(
        (over_quota.status, over_quota.json()["code"].clone()),
        (507, "quota_exceeded".into())
    );
    assert_eq!(reserved_after, reserved_before);
    assert_eq!(bad_hash.status, 422, "{bad_hash:?}");
    assert_eq!(indexes(&state["need_blocks"]), [0, 2, 3]);
    assert_eq!(early.status, 409, "{early:?}");
    assert_eq!(indexes(&early.json()["need_blocks"]), [0, 2, 3]);
    assert!((400..500).contains(&bad_name.status), "{bad_name:?}");
    let listed: Vec<Value> = pipeline.children(folder);
    assert!(
        listed.iter().all(|node| node["name"] != "bad/name"),
        "{listed:?}"
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_a_commit_of_a_deleting_account_is_refused_as_account_not_active() {
    // Arrange: the device's principal stays cached for `session_cache_ttl` (its 3 s maximum), so a commit sent
    // within it passes authentication and meets the guard inside the commit transaction.
    let pipeline = Pipeline::start(&[("session_cache_ttl", "3s")]).await;
    let folder = pipeline.folder("ac4");
    let file = LargeFile::jpeg("photo.jpg", 3 * MIB, 8);
    let upload = pipeline.started(folder, &file);
    pipeline
        .put_blocks(upload, &file, &all_blocks(&file), LANES)
        .await;
    let blob = pipeline.session_blob(upload).await.expect("the session");
    pipeline
        .arrange("UPDATE accounts SET state = 'deleting' WHERE id = $1")
        .await;

    // Act
    let refused = pipeline.commit_upload(upload, folder, &file);

    // Assert
    assert_eq!(refused.json()["code"], "account_not_active", "{refused:?}");
    assert_eq!(pipeline.revision_count().await, 0);
    assert_eq!(pipeline.job_count().await, 0);
    assert_eq!(
        pipeline.blob_state(blob).await.as_deref(),
        Some("uploading")
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_a_jpeg_commit_runs_the_blob_and_media_effects_in_its_transaction_before_the_feed_stamp()
 {
    // Arrange: one JPEG committed; a second one prepared, then the account's feed row removed so the stamp, last
    // in the transaction, fails.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac4");
    let first = LargeFile::jpeg("first.jpg", 3 * MIB, 9);
    let second = LargeFile::jpeg("second.jpg", 3 * MIB, 10);
    let first_upload = pipeline.started(folder, &first);
    let second_upload = pipeline.started(folder, &second);
    for (upload, file) in [(first_upload, &first), (second_upload, &second)] {
        pipeline
            .put_blocks(upload, file, &all_blocks(file), LANES)
            .await;
    }
    let first_blob = pipeline
        .session_blob(first_upload)
        .await
        .expect("a session");
    let second_blob = pipeline
        .session_blob(second_upload)
        .await
        .expect("a session");

    // Act
    let committed = pipeline.commit_upload(first_upload, folder, &first);
    let jobs_after_commit = pipeline.job_count().await;
    pipeline
        .arrange("DELETE FROM account_feeds WHERE account_id = $1")
        .await;
    let rolled_back = pipeline.commit_upload(second_upload, folder, &second);

    // Assert
    assert_eq!(committed.status, 200, "{committed:?}");
    let revision = id(&committed.json(), "revision_id");
    assert_eq!(pipeline.job_kinds(revision).await, ["stills"]);
    assert_eq!(jobs_after_commit, 1);
    assert_eq!(
        pipeline.blob_state(first_blob).await.as_deref(),
        Some("committed")
    );
    assert_eq!(rolled_back.status, 500, "{rolled_back:?}");
    assert_eq!(pipeline.job_count().await, 1);
    assert_eq!(pipeline.revision_count().await, 1);
    assert_eq!(
        pipeline.blob_state(second_blob).await.as_deref(),
        Some("uploading")
    );
    assert!(pipeline.session_blob(second_upload).await.is_some());
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac5_the_scheduled_reaper_removes_an_idle_session_and_releases_its_reservation() {
    // Arrange
    let pipeline = Pipeline::start(&SHORT_SESSIONS).await;
    let folder = pipeline.folder("ac5");
    let baseline = pipeline.reserved_bytes().await;
    let file = LargeFile::new("idle.bin", 4 * BLOCK_SIZE, 11);
    let upload = pipeline.started(folder, &file);
    assert_eq!(pipeline.put_block(upload, &file, 0).status, 204);
    let blob = pipeline.session_blob(upload).await.expect("the session");
    let reserved_while_open = pipeline.reserved_bytes().await;

    // Act
    pipeline
        .wait_session_gone(upload, Instant::now() + EXPIRY_BOUND)
        .await;

    // Assert
    assert_eq!(
        reserved_while_open,
        baseline + i64::try_from(file.bytes.len()).expect("small")
    );
    assert_eq!(pipeline.reserved_bytes().await, baseline);
    assert_eq!(pipeline.blob_state(blob).await, None);
    pipeline.finish().await;
}

/// The settings of AC-68: short sessions plus the orphan sweep at its minimum age, every 2 s.
const CRASH_SETTINGS: [(&str, &str); 4] = [
    SHORT_SESSIONS[0],
    SHORT_SESSIONS[1],
    ("orphan_sweep_age", "1m"),
    ("orphan_sweep_interval", "2s"),
];

/// After the restart: the expired session is reaped, its file swept and its reservation released, and no revision
/// references a missing blob.
async fn assert_uncommitted_upload_cleaned(pipeline: &Pipeline, upload: Uuid, blob: Uuid) {
    pipeline
        .wait_session_gone(upload, Instant::now() + EXPIRY_BOUND)
        .await;
    pipeline
        .wait_file_gone(blob, Instant::now() + EXPIRY_BOUND)
        .await;
    let swept = pipeline
        .server
        .lines()
        .iter()
        .any(|line| line.contains("orphan blob file"));
    eprintln!(
        "AC-68: the blob file was removed by the {}",
        if swept {
            "orphan sweep"
        } else {
            "session reaper"
        }
    );
    assert_eq!(pipeline.blob_state(blob).await, None);
    assert_eq!(pipeline.reserved_bytes().await, 0);
    assert_eq!(pipeline.revision_count().await, 0);
    assert_eq!(pipeline.dangling_revisions().await, Vec::<String>::new());
}

/// A pipeline with an upload of 4 acknowledged blocks; returns its folder, file, session and blob.
async fn acknowledged_upload(name: &str) -> (Pipeline, Uuid, LargeFile, Uuid, Uuid) {
    let pipeline = Pipeline::start(&CRASH_SETTINGS).await;
    let folder = pipeline.folder(name);
    let file = LargeFile::new("crashed.bin", 4 * BLOCK_SIZE, 12);
    let upload = pipeline.started(folder, &file);
    pipeline
        .put_blocks(upload, &file, &all_blocks(&file), LANES)
        .await;
    let blob = pipeline.session_blob(upload).await.expect("the session");
    (pipeline, folder, file, upload, blob)
}

#[tokio::test(flavor = "multi_thread")]
async fn ac68_sigkill_before_the_commit_leaves_no_dangling_revision_and_the_upload_is_cleaned_up() {
    // Arrange
    let (mut pipeline, _, _, upload, blob) = acknowledged_upload("ac68-before").await;
    assert!(pipeline.blob_file(blob).is_file());

    // Act
    pipeline.crash_and_restart();

    // Assert
    assert_uncommitted_upload_cleaned(&pipeline, upload, blob).await;
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac68_sigkill_during_the_commit_leaves_either_a_whole_revision_or_a_cleaned_up_upload() {
    // Arrange
    let (mut pipeline, folder, file, upload, blob) = acknowledged_upload("ac68-during").await;
    let body = Pipeline::commit_body(upload, folder, &file);

    // Act: the commit request is written, the process killed before its answer is read.
    let in_flight = pipeline.send_unanswered("POST", "/api/v1/commits", &body);
    pipeline.crash_and_restart();
    drop(in_flight);

    // Assert
    if pipeline.revision_count().await == 0 {
        eprintln!("AC-68 during commit: the commit did not survive the kill");
        assert_uncommitted_upload_cleaned(&pipeline, upload, blob).await;
    } else {
        eprintln!("AC-68 during commit: the commit survived the kill");
        assert_eq!(pipeline.dangling_revisions().await, Vec::<String>::new());
        assert_eq!(pipeline.reserved_bytes().await, 0);
        let listed = pipeline.children(folder);
        let node = id(&listed[0], "node_id");
        assert!(
            pipeline.download(node) == file.bytes,
            "the stored file differs"
        );
    }
    pipeline.finish().await;
}

/// The F4 joins: each operation, the non-test file that calls or registers it, and what that file must contain.
const CALLERS: &[Caller] = &[
    (
        "UploadService::{start_upload, put_block, get_upload, abort_upload} from the upload routes",
        "components/11_http_api/src/registry/transfer/uploads.rs",
        &[
            ".start_upload(user.account, request)",
            ".put_block(user.account, upload, index, hash, byte_stream(body), length)",
            ".get_upload(user.account, upload)",
            ".abort_upload(user.account, upload)",
        ],
    ),
    (
        "CommitService::commit_upload from the commit route",
        "components/11_http_api/src/registry/transfer/commits.rs",
        &[".commit_upload(actor(&user), request)"],
    ),
    (
        "TreeCommit::publish with the blob effect and the injected effects",
        "components/03_transfer/src/commit.rs",
        &[
            "let effect = BlobCommitEffect {",
            "vec![Arc::new(effect)]",
            "effects.extend(self.commit_effects.iter().cloned())",
            ".publish(actor, vec![entry], effects, now)",
        ],
    ),
    (
        "QuotaLedger::reserve and BlobWriter::create (`create_file`) at the start",
        "components/03_transfer/src/upload.rs",
        &[
            ".reserve(tx, account, request.declared_size)",
            "self.writer.create_file(plan, size, true)",
        ],
    ),
    (
        "BlobWriter::write_block for a block",
        "components/03_transfer/src/put_block.rs",
        &[".write_block(handle, index, block.into())"],
    ),
    (
        "BlobWriter::write_complete (the one-request path of 03)",
        "components/03_transfer/src/stage.rs",
        &[".write_complete(account, size, one_chunk(bytes))"],
    ),
    (
        "AccountGuard::ensure_active first, FeedStamp::stamp last, FeedNotifier::notify after COMMIT",
        "components/02_tree/src/publish.rs",
        &[
            ".ensure_active(tx, account)",
            "self.stamp.stamp(tx, account, touched)",
            "self.notifier.notify(account, seq)",
        ],
    ),
    (
        "QuotaLedger::{adjust_used, release_reservation} in the commit",
        "components/02_tree/src/publish_usage.rs",
        &[
            ".adjust_used(tx, account, usage.delta)",
            ".release_reservation(tx, account, entry.revision.reserved_bytes)",
        ],
    ),
    (
        "the ports of 02 and 03 bound in the composition root",
        "server/src/root/domain.rs",
        &[
            "identity.accounts.clone()",
            "quota.ledger.clone()",
            "feed.stamp.clone()",
            "feed.notifier.clone()",
            "guard: identity.accounts.clone()",
            "commit: tree.service.clone()",
            "writer: blobs.writer.clone()",
            "commit_effects: items(&effects.0)",
        ],
    ),
    (
        "the media effect and the scheduler registrations of SessionReaper and OrphanSweeper",
        "server/src/root/lists.rs",
        &[
            "MediaEnqueueEffect::new(",
            "Arc::new(SessionReaper::new(transfer.service.clone()))",
            "OrphanSweeper::new(&blobs.config",
            "every(Arc::new(orphans), blobs.config.orphan_sweep_interval)",
        ],
    ),
    (
        "the routes bound to the upload service",
        "server/src/root/api.rs",
        &[
            "uploads: transfer.service.clone()",
            "commits: transfer.service.clone()",
        ],
    ),
];

#[test]
fn ac6_every_f4_operation_has_a_production_caller_and_both_janitors_are_scheduled() {
    // Arrange
    let callers = CALLERS;

    // Act
    let missing = missing_callers(callers);

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
}
