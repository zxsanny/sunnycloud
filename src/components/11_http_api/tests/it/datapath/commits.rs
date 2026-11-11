use contracts::{BatchEntryResult, BatchResult, CommitServiceError, Committed, EntryError};
use sc_error::ErrorCode;
use serde_json::json;
use uuid::Uuid;

use super::support::*;
use crate::auth::support::{DEVICE_ACCOUNT, MEMBER_ACCOUNT};

const COMMIT: &str = "CommitService::commit_upload";
const BATCH: &str = "CommitService::commit_batch";

fn commit_body() -> String {
    json!({
        "upload_id": UPLOAD,
        "parent_id": FOLDER,
        "name": "movie.mkv",
        "blocklist": [hex(1), hex(2)],
        "base_revision": BASE,
        "client_mtime": "2026-11-05T10:00:00.5Z",
        "client_meta": {"asset": 7},
        "import_hint": true
    })
    .to_string()
}

fn batch_body() -> String {
    json!({
        "commit_id": COMMIT_ID,
        "entries": [
            {"staged_key": STAGED, "parent_id": FOLDER, "name": "a.txt"},
            {
                "staged_key": COPY,
                "parent_id": FOLDER,
                "name": "b.txt",
                "base_revision": BASE,
                "client_mtime": "2026-11-05T10:00:00Z",
                "client_meta": {"k": 1},
                "import_hint": false
            }
        ]
    })
    .to_string()
}

fn committed(copy: Option<Uuid>) -> Result<Committed, CommitServiceError> {
    Ok(Committed {
        node_id: NODE,
        revision_id: REVISION,
        conflict_copy: copy,
    })
}

#[tokio::test]
async fn ac4_the_device_label_reaches_both_commits() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(COMMIT, committed(None));
    stack.script.push(
        BATCH,
        Ok::<_, CommitServiceError>(BatchResult {
            per_entry: Vec::new(),
        }),
    );

    // Act
    let single = stack
        .as_device("POST", "/api/v1/commits", &commit_body())
        .await;
    let batch = stack
        .as_device("POST", "/api/v1/commits:batch", &batch_body())
        .await;

    // Assert
    assert_eq!((single.status, batch.status), (200, 200));
    let actor = format!("CommitActor {{ account_id: {DEVICE_ACCOUNT}, device_label: \"laptop\" }}");
    let logged = stack.script.log();
    assert_eq!(logged.len(), 2);
    assert!(
        logged[0].starts_with(&format!("{COMMIT} ({actor}, ")),
        "{}",
        logged[0]
    );
    assert!(
        logged[1].starts_with(&format!("{BATCH} ({actor}, ")),
        "{}",
        logged[1]
    );
    stack.stop().await;
}

#[tokio::test]
async fn ac1_a_commit_passes_exactly_what_the_request_holds_and_answers_what_the_port_returned() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(COMMIT, committed(None));
    stack.script.push(COMMIT, committed(Some(COPY)));

    // Act
    let published = stack
        .as_member("POST", "/api/v1/commits", &commit_body())
        .await;
    let conflict = stack
        .as_member("POST", "/api/v1/commits", &commit_body())
        .await;

    // Assert
    assert_eq!(
        published.json(),
        json!({"outcome": "published", "node_id": NODE.to_string(), "revision_id": REVISION.to_string()})
    );
    assert_eq!(
        conflict.json(),
        json!({
            "outcome": "conflict_copy",
            "node_id": NODE.to_string(),
            "revision_id": REVISION.to_string(),
            "copy_node_id": COPY.to_string()
        })
    );
    let request = format!(
        "CommitUpload {{ upload_id: {UPLOAD}, parent_id: {FOLDER}, name: \"movie.mkv\", base_revision: Some({BASE}), blocklist: [{:?}, {:?}], client_mtime: Some(Timestamp(1793872800500000)), client_meta: Some(\"{{\\\"asset\\\":7}}\"), import_hint: true }}",
        hash32(1),
        hash32(2)
    );
    let actor = format!("CommitActor {{ account_id: {MEMBER_ACCOUNT}, device_label: \"test\" }}");
    let line = format!("{COMMIT} ({actor}, {request})");
    assert_eq!(stack.log(COMMIT), [line.clone(), line]);
    stack.stop().await;
}

#[tokio::test]
async fn ac1_a_batch_passes_its_entries_in_order_and_answers_one_result_per_entry() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        BATCH,
        Ok::<_, CommitServiceError>(BatchResult {
            per_entry: vec![
                BatchEntryResult::Committed(Committed {
                    node_id: NODE,
                    revision_id: REVISION,
                    conflict_copy: Some(COPY),
                }),
                BatchEntryResult::Failed(EntryError::EditedSince),
                BatchEntryResult::Failed(EntryError::EffectFailed(ErrorCode::QuotaExceeded)),
            ],
        }),
    );

    // Act
    let batch = stack
        .as_member("POST", "/api/v1/commits:batch", &batch_body())
        .await;

    // Assert
    assert_eq!(batch.status, 200);
    assert_eq!(
        batch.json(),
        json!({"entries": [
            {
                "status": "committed",
                "node_id": NODE.to_string(),
                "revision_id": REVISION.to_string(),
                "conflict_copy": COPY.to_string()
            },
            {"status": "entry_error", "code": "edited_since"},
            {"status": "entry_error", "code": "quota_exceeded"}
        ]})
    );
    let request = format!(
        "BatchCommit {{ commit_id: {COMMIT_ID}, entries: [BatchEntry {{ staged_key: {STAGED}, parent_id: {FOLDER}, name: \"a.txt\", base_revision: None, client_mtime: None, client_meta: None, import_hint: false }}, BatchEntry {{ staged_key: {COPY}, parent_id: {FOLDER}, name: \"b.txt\", base_revision: Some({BASE}), client_mtime: Some(Timestamp(1793872800000000)), client_meta: Some(\"{{\\\"k\\\":1}}\"), import_hint: false }}] }}"
    );
    assert_eq!(
        stack.log(BATCH),
        [format!(
            "{BATCH} (CommitActor {{ account_id: {MEMBER_ACCOUNT}, device_label: \"test\" }}, {request})"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_refusals_of_03_keep_their_codes_and_need_blocks_lists_the_indexes() {
    // Arrange
    let stack = Stack::start().await;
    let script = &stack.script;
    script.push(
        COMMIT,
        Err::<Committed, _>(CommitServiceError::NeedBlocks(vec![1, 4])),
    );
    script.push(COMMIT, Err::<Committed, _>(CommitServiceError::EditedSince));
    script.push(
        COMMIT,
        Err::<Committed, _>(CommitServiceError::BlocklistMismatch),
    );
    script.push(
        COMMIT,
        Err::<Committed, _>(CommitServiceError::UploadNotFound),
    );
    script.push(COMMIT, Err::<Committed, _>(CommitServiceError::ParentGone));
    script.push(
        BATCH,
        Err::<BatchResult, _>(CommitServiceError::BatchTooLarge),
    );
    script.push(
        BATCH,
        Err::<BatchResult, _>(CommitServiceError::CommitIdReused),
    );

    // Act
    let mut answers = Vec::new();
    for _ in 0..5 {
        answers.push(
            stack
                .as_member("POST", "/api/v1/commits", &commit_body())
                .await,
        );
    }
    for _ in 0..2 {
        answers.push(
            stack
                .as_member("POST", "/api/v1/commits:batch", &batch_body())
                .await,
        );
    }

    // Assert
    let seen: Vec<(u16, String)> = answers.iter().map(|a| (a.status, a.code())).collect();
    let expected = [
        (409, "need_blocks"),
        (409, "edited_since"),
        (422, "blocklist_mismatch"),
        (410, "upload_session_gone"),
        (410, "parent_deleted"),
        (422, "batch_too_large"),
        (422, "commit_id_reused"),
    ];
    let expected: Vec<(u16, String)> = expected
        .iter()
        .map(|(s, c)| (*s, (*c).to_owned()))
        .collect();
    assert_eq!(seen, expected);
    assert_eq!(answers[0].json()["need_blocks"], json!([1, 4]));
    stack.stop().await;
}

#[tokio::test]
async fn a_commit_that_does_not_bind_never_reaches_a_port() {
    // Arrange
    let stack = Stack::start().await;
    let with = |change: fn(&mut serde_json::Value)| {
        let mut body: serde_json::Value = serde_json::from_str(&commit_body()).expect("json");
        change(&mut body);
        body.to_string()
    };
    let bad_commits = [
        with(|b| b["blocklist"] = json!(["zz"])),
        with(|b| b["blocklist"] = json!("none")),
        with(|b| b["upload_id"] = json!("7")),
        with(|b| b["account_id"] = json!(FOLDER)),
        with(|b| b["client_mtime"] = json!("yesterday")),
        with(|b| b["import_hint"] = json!("yes")),
        with(|b| {
            b.as_object_mut().map(|o| o.remove("name"));
        }),
        "[]".to_owned(),
        "not json".to_owned(),
    ];
    let bad_batches = [
        json!({"commit_id": COMMIT_ID}).to_string(),
        json!({"commit_id": "7", "entries": []}).to_string(),
        json!({"commit_id": COMMIT_ID, "entries": [{"staged_key": STAGED}]}).to_string(),
        json!({"commit_id": COMMIT_ID, "entries": [{"staged_key": STAGED, "parent_id": FOLDER, "name": "a", "file_hash": hex(1)}]}).to_string(),
    ];

    // Act
    let mut answers = Vec::new();
    for body in &bad_commits {
        answers.push(stack.as_member("POST", "/api/v1/commits", body).await);
    }
    for body in &bad_batches {
        answers.push(stack.as_member("POST", "/api/v1/commits:batch", body).await);
    }

    // Assert
    for answer in &answers {
        assert_eq!(
            answer.status,
            400,
            "{}",
            String::from_utf8_lossy(&answer.body)
        );
        assert_eq!(answer.code(), "invalid_request");
    }
    assert_eq!(stack.script.log(), Vec::<String>::new());
    stack.stop().await;
}
