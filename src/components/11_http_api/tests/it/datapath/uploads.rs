use contracts::{BlockAck, Staged, StartUploadOutcome, Started, UploadError, UploadState};
use sc_clock::Timestamp;
use serde_json::json;

use super::support::*;
use crate::auth::support::{DEVICE_ACCOUNT, MEMBER_ACCOUNT};
use crate::limits::support::{Raw, body_of, status_of};

const START: &str = "UploadService::start_upload";
const GET: &str = "UploadService::get_upload";
const ABORT: &str = "UploadService::abort_upload";
const PUT_BLOCK: &str = "UploadService::put_block";
const STAGE: &str = "UploadService::stage_small";

const EXPIRES: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);

fn start_body() -> String {
    json!({
        "parent_id": FOLDER,
        "name": "movie.mkv",
        "declared_size": 5_000_000,
        "client_key": "key-1",
        "content_hash": hex(0xAB),
        "base_revision": BASE,
        "client_meta": {"device": "laptop"}
    })
    .to_string()
}

fn started() -> Result<StartUploadOutcome, UploadError> {
    Ok(StartUploadOutcome::Started(Started {
        upload_id: UPLOAD,
        need_blocks: vec![0, 1, 2],
        block_size: 2_097_152,
        lane_limit: 4,
        expires_at: EXPIRES,
    }))
}

#[tokio::test]
async fn ac1_every_upload_operation_makes_one_port_call_for_the_account_of_the_session() {
    // Arrange
    let stack = Stack::start().await;
    let script = &stack.script;
    script.push(START, started());
    script.push(
        START,
        Ok::<_, UploadError>(StartUploadOutcome::NothingToSend { revision: REVISION }),
    );
    script.push(
        GET,
        Ok::<_, UploadError>(UploadState {
            need_blocks: vec![2],
            received_count: 2,
            expires_at: EXPIRES,
        }),
    );
    script.push(ABORT, Ok::<(), UploadError>(()));
    script.push(PUT_BLOCK, Ok::<_, UploadError>(BlockAck));
    script.push(
        STAGE,
        Ok::<_, UploadError>(Staged {
            staged_key: STAGED,
            size: 5,
        }),
    );
    let block = format!("/api/v1/uploads/{UPLOAD}/blocks/3");
    let hash = hex(7);
    let block_headers = [("x-blake3", hash.as_str())];
    let staged = format!("/api/v1/staged-uploads/{STAGED}");

    // Act
    let started = stack
        .as_member("POST", "/api/v1/uploads", &start_body())
        .await;
    let nothing = stack
        .as_member("POST", "/api/v1/uploads", &start_body())
        .await;
    let state = stack
        .as_member("GET", &format!("/api/v1/uploads/{UPLOAD}"), "")
        .await;
    let abort = stack
        .as_member("DELETE", &format!("/api/v1/uploads/{UPLOAD}"), "")
        .await;
    let put = stack
        .with_token(
            crate::auth::support::MEMBER_TOKEN,
            "PUT",
            &block,
            &block_headers,
            b"12345",
        )
        .await;
    let stage = stack
        .with_token(
            crate::auth::support::MEMBER_TOKEN,
            "PUT",
            &staged,
            &block_headers,
            b"12345",
        )
        .await;

    // Assert
    assert_eq!(started.status, 200);
    assert_eq!(
        started.json(),
        json!({
            "outcome": "session",
            "upload_id": UPLOAD.to_string(),
            "need_blocks": [0, 1, 2],
            "block_size": 2_097_152,
            "lane_limit": 4,
            "expires_at": "2026-11-06T09:11:12.123456Z"
        })
    );
    assert_eq!(
        nothing.json(),
        json!({"outcome": "nothing_to_send", "revision_id": REVISION.to_string()})
    );
    assert_eq!(
        state.json(),
        json!({
            "upload_id": UPLOAD.to_string(),
            "need_blocks": [2],
            "received_count": 2,
            "expires_at": "2026-11-06T09:11:12.123456Z"
        })
    );
    assert_eq!((abort.status, abort.body.len()), (204, 0));
    assert_eq!((put.status, put.body.len()), (204, 0));
    assert_eq!(stage.status, 200);
    assert_eq!(
        stage.json(),
        json!({"staged_key": STAGED.to_string(), "size": 5})
    );
    let account = MEMBER_ACCOUNT;
    let meta = r#"Some("{\"device\":\"laptop\"}")"#;
    let request = format!(
        "StartUpload {{ parent_id: {FOLDER}, name: \"movie.mkv\", declared_size: 5000000, client_key: \"key-1\", content_hash: Some({:?}), base_revision: Some({BASE}), client_meta: {meta} }}",
        file_hash(0xAB)
    );
    assert_eq!(
        stack.script.log(),
        [
            format!("{START} ({account}, {request})"),
            format!("{START} ({account}, {request})"),
            format!("{GET} ({account}, {UPLOAD})"),
            format!("{ABORT} ({account}, {UPLOAD})"),
            format!(
                "{PUT_BLOCK} ({account}, {UPLOAD}, 3, {}, length=5, bytes=5)",
                hex(7)
            ),
            format!(
                "{STAGE} ({account}, {STAGED}, hash={}, length=5, bytes=5)",
                hex(7)
            ),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_uploads_for_its_own_account_and_a_zero_byte_staged_file_needs_no_hash() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        STAGE,
        Ok::<_, UploadError>(Staged {
            staged_key: STAGED,
            size: 0,
        }),
    );

    // Act
    let staged = stack
        .as_device("PUT", &format!("/api/v1/staged-uploads/{STAGED}"), "")
        .await;

    // Assert
    assert_eq!(staged.status, 200);
    assert_eq!(staged.json()["size"], 0);
    assert_eq!(
        stack.log(STAGE),
        [format!(
            "{STAGE} ({DEVICE_ACCOUNT}, {STAGED}, hash=none, length=0, bytes=0)"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_refusals_of_03_keep_their_codes() {
    // Arrange
    let stack = Stack::start().await;
    let script = &stack.script;
    let errors = [
        UploadError::InvalidName,
        UploadError::ParentNotFound,
        UploadError::SizeTooLarge,
        UploadError::QuotaExceeded,
        UploadError::SessionCapReached,
        UploadError::FreeSpaceFloor,
        UploadError::AccountNotActive,
    ];
    for error in errors {
        script.push(START, Err::<StartUploadOutcome, _>(error));
    }
    script.push(GET, Err::<UploadState, _>(UploadError::UploadNotFound));
    script.push(PUT_BLOCK, Err::<BlockAck, _>(UploadError::HashMismatch));
    script.push(PUT_BLOCK, Err::<BlockAck, _>(UploadError::LengthMismatch));
    script.push(PUT_BLOCK, Err::<BlockAck, _>(UploadError::BlockConflict));
    script.push(PUT_BLOCK, Err::<BlockAck, _>(UploadError::Overloaded));
    script.push(PUT_BLOCK, Err::<BlockAck, _>(UploadError::UploadNotFound));
    script.push(STAGE, Err::<Staged, _>(UploadError::StagedCapReached));
    let hash = hex(7);
    let headers = [("x-blake3", hash.as_str())];
    let block = format!("/api/v1/uploads/{UPLOAD}/blocks/0");
    let token = crate::auth::support::MEMBER_TOKEN;

    // Act
    let mut answers = Vec::new();
    for _ in 0..7 {
        answers.push(
            stack
                .as_member("POST", "/api/v1/uploads", &start_body())
                .await,
        );
    }
    answers.push(
        stack
            .as_member("GET", &format!("/api/v1/uploads/{UPLOAD}"), "")
            .await,
    );
    for _ in 0..5 {
        answers.push(stack.with_token(token, "PUT", &block, &headers, b"x").await);
    }
    answers.push(
        stack
            .with_token(
                token,
                "PUT",
                &format!("/api/v1/staged-uploads/{STAGED}"),
                &headers,
                b"x",
            )
            .await,
    );

    // Assert
    let seen: Vec<(u16, String)> = answers.iter().map(|a| (a.status, a.code())).collect();
    let expected = [
        (422, "invalid_name"),
        (404, "parent_not_found"),
        (413, "size_too_large"),
        (507, "quota_exceeded"),
        (409, "session_cap_exceeded"),
        (507, "free_space_floor"),
        (403, "account_not_active"),
        (410, "upload_session_gone"),
        (422, "block_hash_mismatch"),
        (422, "block_length_mismatch"),
        (409, "block_conflict"),
        (503, "overloaded"),
        (410, "upload_session_gone"),
        (409, "staged_cap_reached"),
    ];
    let expected: Vec<(u16, String)> = expected
        .iter()
        .map(|(s, c)| (*s, (*c).to_owned()))
        .collect();
    assert_eq!(seen, expected);
    stack.stop().await;
}

#[tokio::test]
async fn a_request_that_does_not_bind_never_reaches_a_port() {
    // Arrange
    let stack = Stack::start().await;
    let token = crate::auth::support::MEMBER_TOKEN;
    let good = hex(7);
    let short = &good[..63];
    let block = |index: &str| format!("/api/v1/uploads/{UPLOAD}/blocks/{index}");

    // Act
    let answers = [
        stack
            .as_member("POST", "/api/v1/uploads", r#"{"parent_id": "7", "name": "x"}"#)
            .await,
        stack
            .as_member(
                "POST",
                "/api/v1/uploads",
                &json!({"parent_id": FOLDER, "name": "x", "declared_size": -1, "client_key": "k"}).to_string(),
            )
            .await,
        stack
            .as_member(
                "POST",
                "/api/v1/uploads",
                &json!({"parent_id": FOLDER, "name": "x", "declared_size": 1, "client_key": "k", "account_id": FOLDER}).to_string(),
            )
            .await,
        stack.as_member("GET", "/api/v1/uploads/not-a-uuid", "").await,
        stack.as_member("DELETE", "/api/v1/uploads/not-a-uuid", "").await,
        stack
            .with_token(token, "PUT", &block("three"), &[("x-blake3", good.as_str())], b"x")
            .await,
        stack
            .with_token(token, "PUT", &block("4294967296"), &[("x-blake3", good.as_str())], b"x")
            .await,
        stack.with_token(token, "PUT", &block("0"), &[], b"x").await,
        stack
            .with_token(token, "PUT", &block("0"), &[("x-blake3", "not-a-hash")], b"x")
            .await,
        stack
            .with_token(token, "PUT", &block("0"), &[("x-blake3", short)], b"x")
            .await,
        stack
            .with_token(token, "PUT", "/api/v1/staged-uploads/not-a-uuid", &[("x-blake3", good.as_str())], b"x")
            .await,
        stack
            .with_token(token, "PUT", &format!("/api/v1/staged-uploads/{STAGED}"), &[("x-blake3", "zz")], b"x")
            .await,
    ];

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

#[tokio::test]
async fn an_upper_case_hash_is_accepted_and_reaches_the_port_as_the_same_bytes() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(PUT_BLOCK, Ok::<_, UploadError>(BlockAck));
    let upper = hex(0xAB).to_uppercase();

    // Act
    let put = stack
        .with_token(
            crate::auth::support::MEMBER_TOKEN,
            "PUT",
            &format!("/api/v1/uploads/{UPLOAD}/blocks/0"),
            &[("x-blake3", upper.as_str())],
            b"x",
        )
        .await;

    // Assert
    assert_eq!(put.status, 204);
    assert_eq!(
        stack.log(PUT_BLOCK),
        [format!(
            "{PUT_BLOCK} ({MEMBER_ACCOUNT}, {UPLOAD}, 0, {}, length=1, bytes=1)",
            hex(0xAB)
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_block_or_a_staged_file_without_a_content_length_is_411_before_the_port() {
    // Arrange
    let stack = Stack::start().await;
    let auth = format!(
        "authorization: Bearer {}\r\n",
        crate::auth::support::MEMBER_TOKEN
    );
    let framing = format!("transfer-encoding: chunked\r\nx-blake3: {}\r\n", hex(7));
    let chunk = b"1\r\nx\r\n0\r\n\r\n";
    let mut answers = Vec::new();

    // Act
    for path in [
        format!("/api/v1/uploads/{UPLOAD}/blocks/0"),
        format!("/api/v1/staged-uploads/{STAGED}"),
    ] {
        let mut raw = Raw::connect(stack.public).await;
        raw.send(&Raw::head("PUT", &path, &framing, &auth)).await;
        raw.send(chunk).await;
        answers.push(raw.finish(crate::harness::EVENT_BOUND).await);
    }

    // Assert
    for answer in &answers {
        assert_eq!(status_of(answer), Some(411));
        assert_eq!(body_of(answer)["code"], "length_required");
    }
    assert_eq!(stack.script.log(), Vec::<String>::new());
    stack.stop().await;
}
