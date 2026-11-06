//! AC-1 of AZ-3970: scripted answers in the wire format of `problem_details.md`.

mod support;

use sc_error::ErrorCode;
use serde_json::{Value, json};
use support::{Answer, Client, Fixture, Reply, Route, Script};
use sync_core::{Mapped, SyncError, map_problem};
use sync_core_proto::{ChangePage, Cursor, ServerInfo, StartOutcome, UploadSession};
use uuid::Uuid;

/// Codes of the `sync_api.md` wire table, one request each.
const WIRE_TABLE: [ErrorCode; 27] = [
    ErrorCode::StagedCapReached,
    ErrorCode::CommitIdReused,
    ErrorCode::BatchTooLarge,
    ErrorCode::SizeTooLarge,
    ErrorCode::AccountNotActive,
    ErrorCode::BlocklistMismatch,
    ErrorCode::ParentNotFound,
    ErrorCode::ParentDeleted,
    ErrorCode::NotAFolder,
    ErrorCode::QuotaExceeded,
    ErrorCode::FreeSpaceFloor,
    ErrorCode::ResyncRequired,
    ErrorCode::InvalidCursor,
    ErrorCode::BlockConflict,
    ErrorCode::BlockHashMismatch,
    ErrorCode::UploadSessionGone,
    ErrorCode::NeedBlocks,
    ErrorCode::NameConflict,
    ErrorCode::EditedSince,
    ErrorCode::Cycle,
    ErrorCode::InvalidName,
    ErrorCode::SessionCapExceeded,
    ErrorCode::NotFound,
    ErrorCode::ContentUnavailable,
    ErrorCode::Unauthenticated,
    ErrorCode::RateLimited,
    ErrorCode::UpgradeRequired,
];

fn problem_for(code: ErrorCode) -> Answer {
    let answer = Answer::problem(code);
    match code {
        ErrorCode::RateLimited | ErrorCode::ServiceUnavailable | ErrorCode::Overloaded => {
            answer.retry_after(3)
        }
        ErrorCode::NeedBlocks => answer.need_blocks(&[1, 4]),
        ErrorCode::ResyncRequired => answer.extra("reason", json!("below_purge_horizon")),
        ErrorCode::UpgradeRequired => answer
            .extra("detail", json!("Please upgrade the app"))
            .extra("min_version", json!("2.1.0"))
            .extra("update_url", json!("https://sunnycloud.test/download")),
        _ => answer,
    }
}

fn server_info() -> ServerInfo {
    ServerInfo {
        block_size: 2 * 1024 * 1024,
        max_lanes: 3,
        small_file_max: 2 * 1024 * 1024,
        batch_max_entries: 500,
        max_staged_per_account: 7,
        longpoll_max_wait: 20,
        min_client_version: "1.2.0".to_owned(),
        api_versions: vec!["v1".to_owned()],
    }
}

fn change_page() -> ChangePage {
    serde_json::from_value(json!({
        "items": [],
        "next_cursor": Cursor::new(1, 150).to_string(),
        "has_more": false
    }))
    .expect("a change page in wire form")
}

fn upload_session() -> StartOutcome {
    StartOutcome::Session(UploadSession {
        upload_id: Uuid::from_u128(0x0190_c5f0_0000_7000_8000_0000_0000_0042),
        need_blocks: vec![0, 1, 2],
        lane_limit: 3,
        expires_at: "2026-11-06T00:00:00Z".to_owned(),
    })
}

async fn connect(fixture: &Fixture) -> Client {
    let config = fixture.client_config(&[&rustls::version::TLS13], &[b"h2"]);
    Client::connect(fixture.addr(), "127.0.0.1", config)
        .await
        .expect("handshake with the fixture")
}

fn json_of(reply: &Reply) -> Value {
    serde_json::from_slice(&reply.body).expect("a JSON body")
}

#[tokio::test]
async fn scripted_success_bodies_come_back_exactly() {
    let script = Script::new()
        .on(Route::ServerInfo, Answer::json(200, &server_info()))
        .on(Route::Changes, Answer::json(200, &change_page()))
        .on(Route::StartUpload, Answer::json(200, &upload_session()));
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;

    let info = client
        .get("/api/v1/server-info")
        .await
        .expect("server-info");
    let page = client
        .get("/api/v1/changes?cursor=1.100")
        .await
        .expect("changes");
    let session = client
        .send(
            "POST",
            "/api/v1/uploads",
            &[],
            bytes::Bytes::from_static(b"{}"),
        )
        .await
        .expect("uploads");

    assert_eq!(info.status, 200);
    assert_eq!(
        serde_json::from_slice::<ServerInfo>(&info.body).expect("info"),
        server_info()
    );
    assert_eq!(
        serde_json::from_slice::<ChangePage>(&page.body).expect("page"),
        change_page()
    );
    assert_eq!(
        serde_json::from_slice::<StartOutcome>(&session.body).expect("session"),
        upload_session()
    );
    let requests = fixture.requests();
    assert_eq!(
        requests.iter().map(|r| r.route).collect::<Vec<_>>(),
        [
            Some(Route::ServerInfo),
            Some(Route::Changes),
            Some(Route::StartUpload)
        ]
    );
    assert_eq!(requests[2].body.as_ref(), b"{}");
}

#[tokio::test]
async fn every_wire_table_code_is_answered_in_problem_details_format() {
    let mut script = Script::new();
    for code in WIRE_TABLE {
        script = script.on(Route::Commit, problem_for(code));
    }
    script = script.on(
        Route::Commit,
        Answer::problem_with("weird_code", 418, "Weird").retry_after(1),
    );
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;

    for code in WIRE_TABLE {
        let reply = client
            .send(
                "POST",
                "/api/v1/commits",
                &[],
                bytes::Bytes::from_static(b"{}"),
            )
            .await
            .expect("an answer");

        let body = json_of(&reply);
        assert_eq!(reply.status, code.status(), "status of {code:?}");
        assert_eq!(
            reply.header("content-type"),
            Some("application/problem+json")
        );
        assert_eq!(
            body["type"],
            format!("urn:sunnycloud:error:{}", code.as_str())
        );
        assert_eq!(body["title"], code.title());
        assert_eq!(body["status"], i64::from(code.status()));
        assert_eq!(body["code"], code.as_str());
        let request_id = body["request_id"].as_str().expect("request_id is text");
        assert!(
            Uuid::parse_str(request_id).is_ok(),
            "request_id {request_id}"
        );
        assert_eq!(reply.header("x-request-id"), Some(request_id));
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        let mapped = map_problem(reply.status, &reply.header_pairs(), &reply.body);
        assert!(
            !matches!(mapped, Mapped::Error(SyncError::Unexpected { .. })),
            "{code:?} must be a row of the wire table, got {mapped:?}"
        );
    }
    let weird = client
        .send("POST", "/api/v1/commits", &[], bytes::Bytes::new())
        .await
        .expect("the weird answer");
    assert_eq!(weird.status, 418);
    assert_eq!(json_of(&weird)["code"], "weird_code");
    assert_eq!(
        map_problem(weird.status, &weird.header_pairs(), &weird.body),
        Mapped::Error(SyncError::Unexpected {
            code: "weird_code".to_owned(),
            request_id: json_of(&weird)["request_id"].as_str().map(str::to_owned),
        })
    );
}

#[tokio::test]
async fn problem_extras_travel_in_body_and_headers() {
    let script = Script::new()
        .on(Route::Commit, problem_for(ErrorCode::RateLimited))
        .on(Route::Commit, problem_for(ErrorCode::NeedBlocks))
        .on(Route::Commit, problem_for(ErrorCode::ResyncRequired))
        .on(Route::Commit, problem_for(ErrorCode::UpgradeRequired));
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;
    let mut replies = Vec::new();
    for _ in 0..4 {
        replies.push(
            client
                .send("POST", "/api/v1/commits", &[], bytes::Bytes::new())
                .await
                .expect("answer"),
        );
    }

    assert_eq!(replies[0].header("retry-after"), Some("3"));
    assert_eq!(json_of(&replies[0])["retry_after"], 3);
    assert_eq!(json_of(&replies[1])["need_blocks"], json!([1, 4]));
    assert_eq!(json_of(&replies[2])["reason"], "below_purge_horizon");
    let upgrade = json_of(&replies[3]);
    assert_eq!(upgrade["min_version"], "2.1.0");
    assert_eq!(upgrade["update_url"], "https://sunnycloud.test/download");
}

#[tokio::test]
async fn unscripted_requests_are_visible_not_invented() {
    let fixture = Fixture::start(Script::new().on(Route::ServerInfo, Answer::no_content())).await;
    let mut client = connect(&fixture).await;

    let scripted = client.get("/api/v1/server-info").await.expect("answer");
    let exhausted = client.get("/api/v1/server-info").await.expect("answer");
    let outside = client.get("/api/v1/openapi.json").await.expect("answer");

    assert_eq!(scripted.status, 204);
    assert_eq!(exhausted.status, 501);
    assert_eq!(json_of(&exhausted)["code"], "unscripted_answer");
    assert_eq!(outside.status, 404);
    let log = fixture.requests();
    assert_eq!(
        log.iter().map(|r| r.scripted).collect::<Vec<_>>(),
        [true, false, false]
    );
}

#[tokio::test]
async fn content_answers_follow_the_range_header() {
    let data = support::seeded_bytes(7, 1000);
    let script = Script::new().always(Route::RevisionContent, Answer::content(data.clone()));
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;
    let path = "/api/v1/revisions/r1/content";

    let full = client.get(path).await.expect("full");
    let tail = client
        .send("GET", path, &[("range", "bytes=990-")], bytes::Bytes::new())
        .await
        .expect("tail");
    let suffix = client
        .send("GET", path, &[("range", "bytes=-3")], bytes::Bytes::new())
        .await
        .expect("suffix");
    let beyond = client
        .send(
            "GET",
            path,
            &[("range", "bytes=5000-")],
            bytes::Bytes::new(),
        )
        .await
        .expect("beyond");

    assert_eq!((full.status, full.body.as_ref()), (200, data.as_ref()));
    assert_eq!(full.header("accept-ranges"), Some("bytes"));
    assert_eq!((tail.status, tail.body.as_ref()), (206, &data[990..]));
    assert_eq!(tail.header("content-range"), Some("bytes 990-999/1000"));
    assert_eq!((suffix.status, suffix.body.as_ref()), (206, &data[997..]));
    assert_eq!(beyond.status, 416);
    assert_eq!(beyond.header("content-range"), Some("bytes */1000"));
}
