use contracts::{Download, DownloadError};

use super::support::*;
use crate::auth::support::{
    ADMIN_ACCOUNT, ADMIN_TOKEN, DEVICE_ACCOUNT, MEMBER_ACCOUNT, MEMBER_TOKEN,
};
use crate::sharing::support::{full, partial, served};

const OPEN: &str = "DownloadService::open_revision_content";

fn options(retained_ok: bool, range: &str) -> String {
    format!(
        "DownloadOptions {{ retained_ok: {retained_ok}, range: {range}, if_range: None, if_none_match: None }}"
    )
}

#[tokio::test]
async fn ac5_retained_revisions_are_opened_for_an_admin_session_only() {
    // Arrange
    let stack = Stack::start().await;
    for _ in 0..3 {
        stack
            .script
            .push(OPEN, served(full("application/pdf", 100)));
    }
    stack
        .script
        .push(OPEN, Err::<Download, _>(DownloadError::NotFound));
    let path = format!("/api/v1/revisions/{REVISION}/content");

    // Act
    let member = stack.with_token(MEMBER_TOKEN, "GET", &path, &[], b"").await;
    let admin = stack.with_token(ADMIN_TOKEN, "GET", &path, &[], b"").await;
    let device = stack.as_device("GET", &path, "").await;
    let missing = stack.with_token(MEMBER_TOKEN, "GET", &path, &[], b"").await;

    // Assert
    assert_eq!((member.status, member.body.len()), (200, 100));
    assert_eq!((admin.status, device.status), (200, 200));
    assert_eq!(
        (missing.status, missing.code().as_str()),
        (404, "not_found")
    );
    let target = format!("Revision({REVISION})");
    assert_eq!(
        stack.log(OPEN),
        [
            format!(
                "{OPEN} ({MEMBER_ACCOUNT}, {target}, {})",
                options(false, "None")
            ),
            format!(
                "{OPEN} ({ADMIN_ACCOUNT}, {target}, {})",
                options(true, "None")
            ),
            format!(
                "{OPEN} ({DEVICE_ACCOUNT}, {target}, {})",
                options(false, "None")
            ),
            format!(
                "{OPEN} ({MEMBER_ACCOUNT}, {target}, {})",
                options(false, "None")
            ),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_content_of_a_node_is_its_current_revision_for_the_account_of_the_caller() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(OPEN, served(full("application/pdf", 10)));
    stack.script.push(OPEN, served(full("application/pdf", 10)));
    stack.script.push(OPEN, served(full("application/pdf", 10)));
    let path = format!("/api/v1/nodes/{NODE}/content");

    // Act
    let get = stack.as_member("GET", &path, "").await;
    let head = stack.as_member("HEAD", &path, "").await;
    let admin = stack.with_token(ADMIN_TOKEN, "GET", &path, &[], b"").await;

    // Assert
    assert_eq!((get.status, get.body.len()), (200, 10));
    assert_eq!(
        (head.status, head.body.len(), head.header("content-length")),
        (200, 0, Some("10"))
    );
    assert_eq!(get.header("cache-control"), Some("private"));
    assert_eq!(admin.status, 200);
    let current = format!("Current({NODE})");
    assert_eq!(
        stack.log(OPEN),
        [
            format!(
                "{OPEN} ({MEMBER_ACCOUNT}, {current}, {})",
                options(false, "None")
            ),
            format!(
                "{OPEN} ({MEMBER_ACCOUNT}, {current}, {})",
                options(false, "None")
            ),
            format!(
                "{OPEN} ({ADMIN_ACCOUNT}, {current}, {})",
                options(false, "None")
            ),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_range_reaches_the_port_and_a_partial_answer_is_a_206() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(OPEN, served(partial(5, 9, 100)));
    stack.script.push(
        OPEN,
        Err::<Download, _>(DownloadError::RangeNotSatisfiable { total_size: 100 }),
    );
    let path = format!("/api/v1/revisions/{REVISION}/content");

    // Act
    let ranged = stack
        .with_token(MEMBER_TOKEN, "GET", &path, &[("range", "bytes=5-9")], b"")
        .await;
    let beyond = stack
        .with_token(MEMBER_TOKEN, "GET", &path, &[("range", "bytes=500-")], b"")
        .await;

    // Assert
    assert_eq!((ranged.status, ranged.body.len()), (206, 5));
    assert_eq!(ranged.header("content-range"), Some("bytes 5-9/100"));
    assert_eq!(
        (beyond.status, beyond.header("content-range")),
        (416, Some("bytes */100"))
    );
    let target = format!("Revision({REVISION})");
    let range = |text: &str| {
        format!(
            "{OPEN} ({MEMBER_ACCOUNT}, {target}, {})",
            options(false, text)
        )
    };
    assert_eq!(
        stack.log(OPEN),
        [
            range("Some(ByteRange { start: 5, end: 9 })"),
            range("Some(ByteRange { start: 500, end: 18446744073709551615 })"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_content_route_with_an_id_that_is_not_a_uuid_never_reaches_03() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let node = stack
        .as_member("GET", "/api/v1/nodes/seven/content", "")
        .await;
    let revision = stack
        .as_member("HEAD", "/api/v1/revisions/seven/content", "")
        .await;

    // Assert
    assert_eq!(node.status, 400);
    assert_eq!(node.code(), "invalid_request");
    assert_eq!(revision.status, 400);
    assert_eq!(stack.script.log(), Vec::<String>::new());
    stack.stop().await;
}
