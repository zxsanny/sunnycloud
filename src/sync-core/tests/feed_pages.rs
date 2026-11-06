//! AC-1 and AC-2 of AZ-3965: every page is checked with P1..P6 before the caller sees it, a valid page comes back
//! unchanged and a `has_more` page is followed by an immediate second poll; the request carries cursor, wait, limit.

mod connect_support;
mod feed_support;
mod request_support;
mod support;
mod tree_support;

use std::time::{Duration, Instant};

use feed_support::{
    connected, connected_with, head_answer, item, ms, page, page_answer, target_param,
};
use support::{Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::Cursor;
use tree_support::seen;

const START: Cursor = Cursor::new(1, 100);

fn violation_pages() -> Vec<(&'static str, sync_core_proto::ChangePage)> {
    vec![
        (
            "P1",
            page(vec![item(105, 5), item(103, 3)], Cursor::new(1, 110), false),
        ),
        ("P2", page(vec![item(100, 4)], Cursor::new(1, 110), false)),
        ("P3", page(vec![item(120, 6)], Cursor::new(1, 110), false)),
        ("P4", page(vec![item(101, 7)], Cursor::new(2, 110), false)),
        ("P5", page(Vec::new(), Cursor::new(1, 99), false)),
        (
            "P6",
            page(vec![item(102, 8), item(102, 8)], Cursor::new(1, 110), false),
        ),
    ]
}

#[tokio::test]
async fn each_of_the_six_invalid_pages_is_refused_with_the_rule_it_breaks() {
    // Arrange
    let pages = violation_pages();
    let mut script = Script::new();
    for (_, invalid) in &pages {
        script = script.on(Route::Changes, page_answer(invalid));
    }
    let (api, _fixture) = connected(script).await;

    for (rule, _) in &pages {
        // Act
        let result = api.poll_changes(START, ms(0), None).await;

        // Assert
        match result {
            Err(SyncError::ProtocolViolation { detail }) => {
                assert!(detail.contains(rule), "{rule} not named in {detail:?}")
            }
            other => panic!("{rule}: expected a protocol violation, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_valid_page_is_returned_unchanged_and_the_next_poll_has_no_client_side_delay() {
    // Arrange
    let first = page(
        (101..=150).map(|seq| item(seq, u128::from(seq))).collect(),
        Cursor::new(1, 150),
        true,
    );
    let second = page(vec![item(151, 151)], Cursor::new(1, 151), false);
    let script = Script::new()
        .on(Route::Changes, page_answer(&first))
        .on(Route::Changes, page_answer(&second));
    // A rest would be at least the 600 ms base: the second poll must come back well before it.
    let (api, fixture) = connected_with(script, 30, |config| sync_core::SyncConfig {
        retry_base: Duration::from_millis(600),
        retry_max: Duration::from_secs(2),
        ..config
    })
    .await;

    // Act
    let got_first = api.poll_changes(START, ms(0), Some(50)).await;
    let started = Instant::now();
    let got_second = api.poll_changes(Cursor::new(1, 150), ms(0), Some(50)).await;
    let elapsed = started.elapsed();

    // Assert
    assert_eq!(got_first, Ok(first));
    assert_eq!(got_second, Ok(second));
    assert!(elapsed < ms(400), "the second poll took {elapsed:?}");
    let targets: Vec<String> = seen(&fixture, Route::Changes)
        .into_iter()
        .map(|request| request.target)
        .collect();
    assert_eq!(
        targets,
        vec![
            "/api/v1/changes?cursor=1.100&wait=0&limit=50",
            "/api/v1/changes?cursor=1.150&wait=0&limit=50",
        ]
    );
}

#[tokio::test]
async fn an_empty_page_with_the_same_cursor_is_a_valid_answer_to_an_expired_wait() {
    // Arrange
    let held = page(Vec::new(), START, false);
    let script = Script::new().on(Route::Changes, page_answer(&held));
    let (api, fixture) = connected(script).await;

    // Act
    let result = api.poll_changes(START, Duration::from_secs(25), None).await;

    // Assert
    assert_eq!(result, Ok(held));
    let request = &seen(&fixture, Route::Changes)[0];
    assert_eq!(target_param(&request.target, "wait").as_deref(), Some("25"));
    assert_eq!(target_param(&request.target, "limit"), None);
}

#[tokio::test]
async fn head_cursor_returns_the_cursor_of_the_head_route() {
    // Arrange
    let script = Script::new().on(Route::ChangesHead, head_answer(Cursor::new(3, 4711)));
    let (api, fixture) = connected(script).await;

    // Act
    let head = api.head_cursor().await;

    // Assert
    assert_eq!(head, Ok(Cursor::new(3, 4711)));
    assert_eq!(
        seen(&fixture, Route::ChangesHead)[0].target,
        "/api/v1/changes/head"
    );
}

#[tokio::test]
async fn a_head_answer_that_is_not_a_cursor_is_a_protocol_violation() {
    // Arrange
    let script = Script::new().on(
        Route::ChangesHead,
        support::Answer::json(200, &serde_json::json!({ "cursor": "not-a-cursor" })),
    );
    let (api, _fixture) = connected(script).await;

    // Act
    let head = api.head_cursor().await;

    // Assert
    assert_eq!(
        head,
        Err(SyncError::ProtocolViolation {
            detail: "malformed response body"
        })
    );
}
