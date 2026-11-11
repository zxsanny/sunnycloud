use std::time::Duration;

use contracts::{
    ChangeFeedError, ChangeItem, ChangePage, Cursor, NodeKind, NodeState, ResyncReason,
    RevalidatorError,
};
use serde_json::json;

use super::support::*;
use crate::auth::support::{DEVICE_ACCOUNT, MEMBER_ACCOUNT};

const READ: &str = "ChangeFeed::read";
const HEAD: &str = "ChangeFeed::head";
const REVALIDATE: &str = "PrincipalRevalidator::revalidate";

fn page() -> ChangePage {
    ChangePage {
        items: vec![
            ChangeItem {
                node_id: NODE,
                parent_id: Some(FOLDER),
                kind: NodeKind::File,
                name: "report.pdf".to_owned(),
                state: NodeState::Live,
                revision_id: Some(REVISION),
                size: Some(2048),
                content_hash: Some(file_hash(0xAB)),
                seq: 18,
            },
            ChangeItem {
                node_id: COPY,
                parent_id: None,
                kind: NodeKind::Folder,
                name: "Old".to_owned(),
                state: NodeState::Tombstone,
                revision_id: None,
                size: None,
                content_hash: None,
                seq: 19,
            },
        ],
        next_cursor: Cursor { epoch: 3, seq: 19 },
        has_more: true,
    }
}

fn revalidated(count: usize, stack: &Stack) {
    for _ in 0..count {
        stack
            .script
            .push(REVALIDATE, Ok::<bool, RevalidatorError>(true));
    }
}

fn principal(account: impl std::fmt::Display, kind: &str, id: u128, label: &str) -> String {
    format!(
        "Principal {{ account_id: {account}, kind: {kind}, session_or_device_id: {}, role: User, label: \"{label}\" }}",
        uuid::Uuid::from_u128(id)
    )
}

#[tokio::test]
async fn ac6_the_feed_gets_the_principal_the_revalidator_the_cursor_the_wait_and_the_limit_and_its_four_outcomes_map()
 {
    // Arrange
    let stack = Stack::start().await;
    revalidated(4, &stack);
    let script = &stack.script;
    script.push(READ, Ok::<_, ChangeFeedError>(page()));
    script.push(
        READ,
        Err::<ChangePage, _>(ChangeFeedError::ResyncRequired {
            reason: ResyncReason::BelowPurgeHorizon,
        }),
    );
    script.push(READ, Err::<ChangePage, _>(ChangeFeedError::BadCursor));
    script.push(READ, Err::<ChangePage, _>(ChangeFeedError::Unauthenticated));
    let path = "/api/v1/changes?cursor=3.17&wait=25&limit=100";

    // Act
    let answers = [
        stack.as_member("GET", path, "").await,
        stack.as_member("GET", path, "").await,
        stack.as_member("GET", path, "").await,
        stack.as_member("GET", path, "").await,
    ];

    // Assert
    let [ok, resync, bad, gone] = &answers;
    assert_eq!(ok.status, 200);
    assert_eq!(ok.header("cache-control"), Some("no-store"));
    assert_eq!(
        ok.json(),
        json!({
            "items": [
                {
                    "node_id": NODE.to_string(),
                    "parent_id": FOLDER.to_string(),
                    "kind": "file",
                    "name": "report.pdf",
                    "state": "live",
                    "revision_id": REVISION.to_string(),
                    "size": 2048,
                    "content_hash": hex(0xAB),
                    "seq": 18
                },
                {
                    "node_id": COPY.to_string(),
                    "parent_id": null,
                    "kind": "folder",
                    "name": "Old",
                    "state": "tombstone",
                    "revision_id": null,
                    "size": null,
                    "content_hash": null,
                    "seq": 19
                }
            ],
            "next_cursor": "3.19",
            "has_more": true
        })
    );
    assert_eq!(
        (resync.status, resync.code().as_str()),
        (410, "resync_required")
    );
    assert_eq!(resync.json()["reason"], "below_purge_horizon");
    assert_eq!((bad.status, bad.code().as_str()), (422, "invalid_cursor"));
    assert_eq!(
        (gone.status, gone.code().as_str()),
        (401, "unauthenticated")
    );
    let session = principal(MEMBER_ACCOUNT, "Session", 0xB1, "test");
    let line = format!(
        "{READ} ({session}, Cursor {{ epoch: 3, seq: 17 }}, Some(25s), Some(100)) revalidated=Ok(true)"
    );
    assert_eq!(
        stack.log(READ),
        [line.clone(), line.clone(), line.clone(), line]
    );
    assert_eq!(stack.script.calls(REVALIDATE), 4);
    stack.stop().await;
}

#[tokio::test]
async fn a_poll_without_wait_or_limit_passes_none_and_a_device_polls_for_its_own_account() {
    // Arrange
    let stack = Stack::start().await;
    revalidated(1, &stack);
    stack.script.push(
        READ,
        Ok::<_, ChangeFeedError>(ChangePage {
            items: Vec::new(),
            next_cursor: Cursor { epoch: 0, seq: 0 },
            has_more: false,
        }),
    );

    // Act
    let empty = stack
        .as_device("GET", "/api/v1/changes?cursor=0.0", "")
        .await;

    // Assert
    assert_eq!(
        empty.json(),
        json!({"items": [], "next_cursor": "0.0", "has_more": false})
    );
    let device = principal(DEVICE_ACCOUNT, "Device", 0xB3, "laptop");
    assert_eq!(
        stack.log(READ),
        [format!(
            "{READ} ({device}, Cursor {{ epoch: 0, seq: 0 }}, None, None) revalidated=Ok(true)"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_cursor_that_is_not_e_dot_s_is_422_and_a_malformed_wait_or_limit_or_no_cursor_is_400_without_a_port_call()
 {
    // Arrange
    let stack = Stack::start().await;
    let cursors = [
        "7",
        "1.",
        ".1",
        "a.b",
        "1.2.3",
        "-1.2",
        "+1.2",
        "1.-2",
        "1.2%20",
        "99999999999999999999.1",
        "",
    ];

    // Act
    let mut bad_cursors = Vec::new();
    for cursor in cursors {
        bad_cursors.push(
            stack
                .as_member("GET", &format!("/api/v1/changes?cursor={cursor}"), "")
                .await,
        );
    }
    let bad_wait = stack
        .as_member("GET", "/api/v1/changes?cursor=1.1&wait=soon", "")
        .await;
    let bad_limit = stack
        .as_member("GET", "/api/v1/changes?cursor=1.1&limit=-5", "")
        .await;
    let none = stack.as_member("GET", "/api/v1/changes", "").await;

    // Assert
    for answer in &bad_cursors {
        assert_eq!(
            (answer.status, answer.code().as_str()),
            (422, "invalid_cursor")
        );
    }
    for answer in [&bad_wait, &bad_limit, &none] {
        assert_eq!(
            (answer.status, answer.code().as_str()),
            (400, "invalid_request")
        );
    }
    assert_eq!(stack.script.log(), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn a_hold_cap_is_a_429_with_retry_after() {
    // Arrange
    let stack = Stack::start().await;
    revalidated(1, &stack);
    stack.script.push(
        READ,
        Err::<ChangePage, _>(ChangeFeedError::TooManyHolds {
            retry_after: Duration::from_secs(5),
        }),
    );

    // Act
    let capped = stack
        .as_member("GET", "/api/v1/changes?cursor=1.1&wait=30", "")
        .await;

    // Assert
    assert_eq!(
        (capped.status, capped.code().as_str()),
        (429, "rate_limited")
    );
    assert_eq!(capped.header("retry-after"), Some("5"));
    stack.stop().await;
}

#[tokio::test]
async fn the_head_is_the_cursor_of_the_account_of_the_caller() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(HEAD, Ok::<_, ChangeFeedError>(Cursor { epoch: 2, seq: 41 }));

    // Act
    let head = stack.as_member("GET", "/api/v1/changes/head", "").await;

    // Assert
    assert_eq!(head.status, 200);
    assert_eq!(head.json(), json!({"cursor": "2.41"}));
    assert_eq!(head.header("cache-control"), Some("no-store"));
    assert_eq!(stack.log(HEAD), [format!("{HEAD} ({MEMBER_ACCOUNT})")]);
    stack.stop().await;
}
