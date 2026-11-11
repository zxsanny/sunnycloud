use contracts::ByteRange;

use super::support::*;

#[tokio::test]
async fn the_download_starts_unless_the_range_begins_after_byte_zero() {
    // Arrange
    let stack = Stack::start().await;
    for download in [
        full("image/png", 1000),
        partial(0, 99, 1000),
        partial(100, 199, 1000),
    ] {
        stack.script.push(AUTHORIZE_CONTENT, allowed());
        stack.script.push(OPEN_REVISION, served(download));
    }

    // Act
    let whole = stack.as_visitor("GET", &link("/content"), &[]).await;
    let head_of_file = stack
        .as_visitor("GET", &link("/content"), &[("range", "bytes=0-99")])
        .await;
    let middle = stack
        .as_visitor("GET", &link("/content"), &[("range", "bytes=100-199")])
        .await;

    // Assert
    assert_eq!(
        (whole.status, head_of_file.status, middle.status),
        (200, 206, 206)
    );
    let starts: Vec<bool> = stack
        .log(AUTHORIZE_CONTENT)
        .iter()
        .map(|line| line.ends_with("starts_download=true"))
        .collect();
    assert_eq!(starts, [true, true, false]);
    stack.stop().await;
}

#[tokio::test]
async fn the_range_and_the_validators_reach_03_and_the_current_revision_of_the_link_is_streamed() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_CONTENT, allowed());
    stack
        .script
        .push(OPEN_REVISION, served(partial(100, 199, 1000)));

    // Act
    let reply = stack
        .as_visitor(
            "GET",
            &link("/content"),
            &[
                ("range", "bytes=100-199"),
                ("if-range", "\"blob-etag\""),
                ("if-none-match", "\"other\""),
            ],
        )
        .await;

    // Assert
    assert_eq!(reply.status, 206);
    assert_eq!(reply.header("content-range"), Some("bytes 100-199/1000"));
    assert_eq!(reply.body.len(), 100);
    let range = ByteRange {
        start: 100,
        end: 199,
    };
    assert_eq!(
        stack.log(OPEN_REVISION),
        [format!(
            "{OPEN_REVISION} ({SHARE_ACCOUNT}, Revision({REVISION}), \
             DownloadOptions {{ retained_ok: false, range: Some({range:?}), \
             if_range: Some(\"\\\"blob-etag\\\"\"), if_none_match: Some(\"\\\"other\\\"\") }})"
        )]
    );
    assert_eq!(
        stack.order(),
        ["PublicShareService::authorize_content", OPEN_REVISION]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_head_is_never_a_download_start() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_CONTENT, allowed());
    stack
        .script
        .push(OPEN_REVISION, served(full("image/png", 1000)));

    // Act
    let reply = stack.as_visitor("HEAD", &link("/content"), &[]).await;

    // Assert
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("content-length"), Some("1000"));
    assert!(reply.body.is_empty());
    assert!(stack.log(AUTHORIZE_CONTENT)[0].ends_with("starts_download=false"));
    stack.stop().await;
}
