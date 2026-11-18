//! AZ-3656 (E-22/p04), flow F5 through the real `sunnycloud serve` binary on PostgreSQL 18 and a blob volume over
//! HTTP: staged small files published by one idempotent `commits:batch` (AC-1), the stored response on a replay
//! (AC-2), per-entry failures (AC-3), the zero-byte file and the batch limit (AC-4), staged expiry from the
//! scheduler (AC-5), and the production callers of every F5 operation (AC-6).
//!
//! AC-1 stages over HTTP/2 (h2c, parallel streams on one connection). Durations are set short through config
//! (`_docs/02_document/tests/environment.md`; the test clock is only for ACs that assert a production default):
//! AC-3 and AC-5 set `upload_session_ttl` to its minimum (1 min) and poll the wall clock until it has passed.

mod pipeline_support;
mod root_support;
mod serve_support;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{File, Pipeline, entries, entry, spread};
use serde_json::Value;
use uuid::Uuid;

/// The smallest `upload_session_ttl` the key table accepts.
const SHORT_TTL: (&str, &str) = ("upload_session_ttl", "1m");

fn names(listing: &[Value]) -> BTreeSet<String> {
    listing
        .iter()
        .map(|node| node["name"].as_str().expect("a name").to_owned())
        .collect()
}

fn statuses(result: &Value) -> Vec<String> {
    result["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| match entry["status"].as_str() {
            Some("entry_error") => entry["code"].as_str().unwrap_or_default().to_owned(),
            other => other.unwrap_or_default().to_owned(),
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_two_hundred_staged_files_publish_in_one_batch_with_one_feed_advance() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac1");
    let files = spread("ac1", 200);
    let seq_before = pipeline.feed_seq();

    // Act
    let staged = pipeline.stage_all(&files).await;
    let committed = pipeline.commit_batch(Uuid::new_v4(), &entries(&files, folder));
    let listing = pipeline.children(folder);
    let seq_after = pipeline.feed_seq();
    let reserved = pipeline.reserved_bytes().await;

    // Assert
    let refused: Vec<_> = staged
        .iter()
        .filter(|answer| answer.status != 200)
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(committed.status, 200, "{committed:?}");
    assert_eq!(statuses(&committed.json()), vec!["committed"; 200]);
    let expected: BTreeSet<(String, u64)> = files
        .iter()
        .map(|file| (file.name.clone(), file.bytes.len() as u64))
        .collect();
    let listed: BTreeSet<(String, u64)> = listing
        .iter()
        .map(|node| {
            let name = node["name"].as_str().expect("a name").to_owned();
            (name, node["size"].as_u64().expect("a size"))
        })
        .collect();
    assert_eq!(listed, expected);
    assert_eq!(seq_after, seq_before + 1);
    assert_eq!(reserved, 0);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_a_replayed_commit_id_returns_the_stored_response_without_a_duplicate_or_conflict_copy()
{
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac2");
    let files: Vec<File> = (0..5)
        .map(|index| File::new(&format!("ac2-{index}.txt"), 1000 + index, index as u64 + 7))
        .collect();
    for answer in pipeline.stage_all(&files).await {
        assert_eq!(answer.status, 200, "{answer:?}");
    }
    let commit_id = Uuid::new_v4();
    let lost = pipeline.commit_batch(commit_id, &entries(&files, folder));
    let seq_after_commit = pipeline.feed_seq();

    // Act
    let retried = pipeline.commit_batch(commit_id, &entries(&files, folder));

    // Assert
    assert_eq!(lost.status, 200, "{lost:?}");
    assert_eq!(statuses(&lost.json()), vec!["committed"; 5]);
    assert_eq!(retried.status, lost.status);
    assert_eq!(retried.json(), lost.json());
    let listing = pipeline.children(folder);
    let expected: BTreeSet<String> = files.iter().map(|file| file.name.clone()).collect();
    assert_eq!(names(&listing), expected);
    assert_eq!(pipeline.feed_seq(), seq_after_commit);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_an_invalid_name_and_an_expired_staged_key_fail_alone_while_the_rest_commit() {
    // Arrange: no reaper cycle within the test, so the expired staged row is still stored when it is committed.
    let pipeline = Pipeline::start(&[SHORT_TTL, ("session_reap_interval", "1h")]).await;
    let folder = pipeline.folder("ac3");
    let expired = File::new("expired.txt", 4096, 31);
    assert_eq!(pipeline.stage(&expired).status, 200);
    let (_, expires_at) = pipeline
        .staged_row(expired.key)
        .await
        .expect("the staged row");
    pipeline.wait_past(expires_at).await;
    let valid: Vec<File> = (0..4)
        .map(|index| File::new(&format!("ac3-{index}.txt"), 2048, index + 40))
        .collect();
    let misnamed = File::new("misnamed", 2048, 50);
    for file in valid.iter().chain([&misnamed]) {
        assert_eq!(pipeline.stage(file).status, 200);
    }
    let batch = vec![
        entry(valid[0].key, folder, &valid[0].name),
        entry(misnamed.key, folder, "bad/name"),
        entry(valid[1].key, folder, &valid[1].name),
        entry(expired.key, folder, &expired.name),
        entry(valid[2].key, folder, &valid[2].name),
        entry(valid[3].key, folder, &valid[3].name),
    ];

    // Act
    let committed = pipeline.commit_batch(Uuid::new_v4(), &batch);

    // Assert
    assert_eq!(committed.status, 200, "{committed:?}");
    assert_eq!(
        statuses(&committed.json()),
        [
            "committed",
            "invalid_name",
            "committed",
            "upload_session_gone",
            "committed",
            "committed"
        ]
    );
    let expected: BTreeSet<String> = valid.iter().map(|file| file.name.clone()).collect();
    assert_eq!(names(&pipeline.children(folder)), expected);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_a_zero_byte_file_commits_through_the_staged_path_and_an_oversized_batch_is_422() {
    // Arrange
    let pipeline = Pipeline::start(&[("batch_max_entries", "50")]).await;
    let folder = pipeline.folder("ac4");
    let empty = File::new("empty.txt", 0, 1);
    let oversized: Vec<Value> = (0..51)
        .map(|index| entry(Uuid::new_v4(), folder, &format!("over-{index}")))
        .collect();

    // Act
    let staged = pipeline.stage(&empty);
    let committed = pipeline.commit_batch(Uuid::new_v4(), &entries(&[empty], folder));
    let too_large = pipeline.commit_batch(Uuid::new_v4(), &oversized);

    // Assert
    assert_eq!(staged.status, 200, "{staged:?}");
    assert_eq!(staged.json()["size"], 0);
    assert_eq!(statuses(&committed.json()), ["committed"]);
    let listing = pipeline.children(folder);
    assert_eq!(listing.len(), 1, "{listing:?}");
    assert_eq!(
        (listing[0]["name"].as_str(), listing[0]["size"].as_u64()),
        (Some("empty.txt"), Some(0))
    );
    assert_eq!(too_large.status, 422, "{too_large:?}");
    assert_eq!(too_large.json()["code"], "batch_too_large");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac5_the_scheduled_reaper_removes_an_expired_staged_file_and_releases_its_reservation() {
    // Arrange
    let pipeline = Pipeline::start(&[SHORT_TTL, ("session_reap_interval", "1s")]).await;
    let baseline = pipeline.reserved_bytes().await;
    let file = File::new("never-committed.bin", 1024 * 1024, 77);
    assert_eq!(pipeline.stage(&file).status, 200);
    let (reserved, expires_at) = pipeline.staged_row(file.key).await.expect("the staged row");
    let reserved_while_staged = pipeline.reserved_bytes().await;

    // Act
    pipeline.wait_past(expires_at).await;
    pipeline
        .wait_staged_gone(file.key, Instant::now() + Duration::from_secs(30))
        .await;

    // Assert
    assert!(reserved > 0);
    assert_eq!(reserved_while_staged, baseline + reserved);
    assert_eq!(pipeline.reserved_bytes().await, baseline);
    pipeline.finish().await;
}

/// The F5 joins: each operation, the non-test file that calls or registers it, and what that file must contain.
const CALLERS: &[Caller] = &[
    (
        "UploadService::stage_small from the staged-upload route",
        "components/11_http_api/src/registry/transfer/uploads.rs",
        &[".stage_small(user.account, staged_key, hash"],
    ),
    (
        "CommitService::commit_batch from the batch route",
        "components/11_http_api/src/registry/transfer/commits.rs",
        &[".commit_batch(actor(&user), request)"],
    ),
    (
        "TreeCommit::publish with the receipt and blob effects",
        "components/03_transfer/src/batch.rs",
        &[
            "let receipt = ReceiptEffect {",
            "let staged = StagedCommitEffect::new(",
            "vec![Arc::new(receipt), Arc::new(staged)]",
            ".publish(actor, entries, effects, now)",
        ],
    ),
    (
        "FeedStamp::stamp once after the per-entry savepoints",
        "components/02_tree/src/publish.rs",
        &["self.stamp.stamp(tx, account, touched)"],
    ),
    (
        "the routes bound to the upload service",
        "server/src/root/api.rs",
        &[
            "uploads: transfer.service.clone()",
            "commits: transfer.service.clone()",
        ],
    ),
    (
        "the tree commit, its feed stamp and quota ledger bound",
        "server/src/root/domain.rs",
        &[
            "stamp: Arc::new(PgFeedStamp)",
            "feed.stamp.clone()",
            "commit: tree.service.clone()",
            "quota: quota.ledger.clone()",
            "registry: blobs.registry.clone()",
        ],
    ),
    (
        "the scheduler registrations of ReceiptJanitor and SessionReaper",
        "server/src/root/lists.rs",
        &[
            "Arc::new(SessionReaper::new(transfer.service.clone()))",
            "Arc::new(ReceiptJanitor::new(transfer.service.clone()))",
        ],
    ),
];

#[test]
fn ac6_every_f5_operation_has_a_production_caller_and_both_janitors_are_scheduled() {
    // Arrange
    let callers = CALLERS;

    // Act
    let missing = missing_callers(callers);

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
}
