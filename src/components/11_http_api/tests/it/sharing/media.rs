use std::collections::HashMap;

use contracts::{
    DerivedBlob, DerivedError, Disposition, Download, DownloadError, ThumbState,
    ThumbnailStateError,
};
use sc_ids::FileHash;
use serde_json::json;

use super::support::*;
use crate::auth::support::{DEVICE_ACCOUNT, MEMBER_ACCOUNT};
use crate::content::support::KIB;

fn thumbnail_path(tail: &str) -> String {
    format!("/api/v1/nodes/{NODE}/thumbnail{tail}")
}

fn cors_headers(reply: &crate::worker::support::Reply) -> Vec<String> {
    reply
        .headers
        .keys()
        .filter(|name| name.as_str().starts_with("access-control-"))
        .map(|name| name.to_string())
        .collect()
}

#[tokio::test]
async fn a_ready_thumbnail_streams_through_03_with_the_content_headers_and_no_cors() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN_DERIVED,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/webp", KIB)));

    // Act
    let reply = stack.as_member("GET", &thumbnail_path(""), "").await;

    // Assert
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("content-type"), Some("image/webp"));
    assert_eq!(reply.header("content-disposition"), Some("inline"));
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(reply.header("cache-control"), Some("private"));
    assert_eq!(reply.header("accept-ranges"), Some("bytes"));
    assert_eq!(reply.body.len(), KIB);
    assert_eq!(cors_headers(&reply), Vec::<String>::new());
    assert_eq!(
        stack.log("DerivedOpener"),
        [format!("{OPEN_DERIVED} ({MEMBER_ACCOUNT}, {NODE}, Thumb)")]
    );
    let blob = format!("{:?}", blob());
    assert_eq!(
        stack.log(OPEN_BLOB),
        [format!(
            "{OPEN_BLOB} ({MEMBER_ACCOUNT}, {blob}, Some(\"image/webp\"), \
             DownloadOptions {{ retained_ok: false, range: None, if_range: None, if_none_match: None }})"
        )]
    );
    assert_eq!(stack.order(), [OPEN_DERIVED, OPEN_BLOB]);
    stack.stop().await;
}

#[tokio::test]
async fn the_type_stored_with_the_derived_blob_is_the_type_handed_to_03() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN_DERIVED,
        Ok::<DerivedBlob, DerivedError>(derived("image/jpeg")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/jpeg", KIB)));

    // Act
    let reply = stack.as_member("GET", &thumbnail_path(""), "").await;

    // Assert
    assert_eq!(reply.status, 200);
    let to_03 = stack.log(OPEN_BLOB);
    assert!(to_03[0].contains("Some(\"image/jpeg\")"), "{to_03:?}");
    stack.stop().await;
}

#[tokio::test]
async fn the_size_picks_the_kind_and_a_device_token_may_read_it_too() {
    // Arrange
    let stack = Stack::start().await;
    for _ in 0..2 {
        stack.script.push(
            OPEN_DERIVED,
            Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
        );
        stack.script.push(OPEN_BLOB, served(full("image/webp", 8)));
    }

    // Act
    let preview = stack
        .as_member("GET", &thumbnail_path("?size=preview"), "")
        .await;
    let device = stack
        .as_device("GET", &thumbnail_path("?size=small"), "")
        .await;
    let large = stack
        .as_member("GET", &thumbnail_path("?size=large"), "")
        .await;

    // Assert
    assert_eq!((preview.status, device.status), (200, 200));
    assert_eq!(
        (large.status, large.code().as_str()),
        (400, "invalid_request")
    );
    assert_eq!(
        stack.log("DerivedOpener"),
        [
            format!("{OPEN_DERIVED} ({MEMBER_ACCOUNT}, {NODE}, Preview)"),
            format!("{OPEN_DERIVED} ({DEVICE_ACCOUNT}, {NODE}, Thumb)"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_thumbnail_that_is_not_ready_or_will_never_exist_answers_its_own_404_code() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN_DERIVED,
        Err::<DerivedBlob, DerivedError>(DerivedError::NotReady),
    );
    stack.script.push(
        OPEN_DERIVED,
        Err::<DerivedBlob, DerivedError>(DerivedError::NotFound),
    );

    // Act
    let pending = stack.as_member("GET", &thumbnail_path(""), "").await;
    let never = stack.as_member("GET", &thumbnail_path(""), "").await;

    // Assert
    assert_eq!(pending.status, 404);
    assert_eq!(pending.code(), "thumbnail_not_ready");
    assert_eq!(never.status, 404);
    assert_eq!(never.code(), "thumbnail_unavailable");
    assert_eq!(stack.log(OPEN_BLOB), Vec::<String>::new());
    assert_eq!(pending.header("x-content-type-options"), Some("nosniff"));
    stack.stop().await;
}

#[tokio::test]
async fn a_transcode_is_ranged_mp4_from_03_and_a_missing_one_is_the_derived_404() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN_DERIVED,
        Ok::<DerivedBlob, DerivedError>(derived("video/mp4")),
    );
    stack.script.push(OPEN_BLOB, served(partial(10, 19, 100)));
    stack.script.push(
        OPEN_DERIVED,
        Err::<DerivedBlob, DerivedError>(DerivedError::NotFound),
    );
    let path = format!("/api/v1/nodes/{NODE}/transcode");

    // Act
    let ranged = stack
        .call(
            "GET",
            &path,
            &[
                (
                    "authorization",
                    bearer(crate::auth::support::MEMBER_TOKEN).as_str(),
                ),
                ("range", "bytes=10-19"),
            ],
            "",
        )
        .await;
    let none = stack.as_member("HEAD", &path, "").await;

    // Assert
    assert_eq!(ranged.status, 206);
    assert_eq!(ranged.header("content-range"), Some("bytes 10-19/100"));
    assert_eq!(none.status, 404);
    let blob_calls = stack.log(OPEN_BLOB);
    assert!(
        blob_calls[0].contains("Some(\"video/mp4\")"),
        "{blob_calls:?}"
    );
    assert!(
        blob_calls[0].contains("start: 10, end: 19"),
        "{blob_calls:?}"
    );
    assert_eq!(
        stack.log(OPEN_DERIVED),
        [
            format!("{OPEN_DERIVED} ({MEMBER_ACCOUNT}, {NODE}, Transcode)"),
            format!("{OPEN_DERIVED} ({MEMBER_ACCOUNT}, {NODE}, Transcode)"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_hostile_derived_type_is_an_attachment_and_a_node_that_is_not_a_uuid_is_400() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN_DERIVED,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack.script.push(
        OPEN_BLOB,
        served(Download {
            disposition: Disposition::Attachment,
            ..full("text/html", 16)
        }),
    );

    // Act
    let hostile = stack.as_member("GET", &thumbnail_path(""), "").await;
    let bad_node = stack
        .as_member("GET", "/api/v1/nodes/seven/thumbnail", "")
        .await;

    // Assert
    assert_eq!(
        hostile.header("content-type"),
        Some("application/octet-stream")
    );
    assert!(
        hostile
            .header("content-disposition")
            .is_some_and(|value| value.starts_with("attachment"))
    );
    assert_eq!(hostile.header("content-security-policy"), Some("sandbox"));
    assert_eq!(bad_node.status, 400);
    assert_eq!(bad_node.code(), "invalid_request");
    stack.stop().await;
}

#[tokio::test]
async fn a_media_route_needs_a_credential_and_03s_not_found_is_the_uniform_404() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN_DERIVED,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack.script.push(
        OPEN_BLOB,
        Err::<Download, DownloadError>(DownloadError::NotFound),
    );

    // Act
    let anonymous = stack.as_visitor("GET", &thumbnail_path(""), &[]).await;
    let gone = stack.as_member("GET", &thumbnail_path(""), "").await;

    // Assert
    assert_eq!(anonymous.status, 401);
    assert_eq!((gone.status, gone.code().as_str()), (404, "not_found"));
    stack.stop().await;
}

#[tokio::test]
async fn only_the_bytes_of_a_public_share_may_answer_a_wildcard_origin_and_never_with_credentials()
{
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_CONTENT, allowed());
    stack
        .script
        .push(OPEN_REVISION, served(full("image/png", 64)));
    stack.script.push(AUTHORIZE_CONTENT, allowed());
    stack
        .script
        .push(OPEN_REVISION, served(full("image/png", 64)));
    stack.script.push(
        AUTHORIZE_CONTENT,
        Err::<contracts::ShareContent, contracts::PublicShareError>(
            contracts::PublicShareError::NotFound,
        ),
    );
    stack.script.push(
        OPEN_DERIVED,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack.script.push(OPEN_BLOB, served(full("image/webp", 64)));
    let foreign = [("origin", "https://elsewhere.example")];

    // Act
    let shared = stack.as_visitor("GET", &link("/content"), &foreign).await;
    let shared_head = stack.as_visitor("HEAD", &link("/content"), &foreign).await;
    let missing = stack.as_visitor("GET", &link("/content"), &foreign).await;
    let thumbnail = stack.as_member("GET", &thumbnail_path(""), "").await;
    let preflight = stack
        .call(
            "OPTIONS",
            &link("/content"),
            &[
                ("origin", "https://elsewhere.example"),
                ("access-control-request-method", "GET"),
            ],
            "",
        )
        .await;

    // Assert
    for reply in [&shared, &shared_head] {
        assert_eq!(reply.status, 200);
        assert_eq!(reply.header("access-control-allow-origin"), Some("*"));
        assert_eq!(reply.header("access-control-allow-credentials"), None);
        assert_eq!(cors_headers(reply), ["access-control-allow-origin"]);
    }
    assert_eq!(missing.status, 404);
    assert_eq!(cors_headers(&missing), Vec::<String>::new());
    assert_eq!(cors_headers(&thumbnail), Vec::<String>::new());
    assert_eq!(preflight.status, 405);
    assert_eq!(cors_headers(&preflight), Vec::<String>::new());
    stack.stop().await;
}

fn hash(byte: u8) -> FileHash {
    FileHash::from_bytes([byte; 32])
}

#[tokio::test]
async fn thumbnail_states_is_one_port_call_and_leaves_out_hashes_with_no_state() {
    // Arrange
    let stack = Stack::start().await;
    let found: HashMap<FileHash, ThumbState> = HashMap::from([
        (hash(1), ThumbState::Ready),
        (hash(2), ThumbState::Pending),
        (hash(3), ThumbState::Failed),
        (hash(4), ThumbState::None),
    ]);
    stack
        .script
        .push(STATES, Ok::<_, ThumbnailStateError>(found));
    let body = json!({"content_hashes": [
        hash(1).to_string(), hash(2).to_string().to_uppercase(), hash(3).to_string(), hash(4).to_string()
    ]});

    // Act
    let reply = stack
        .as_device("POST", "/api/v1/thumbnail-states", &body.to_string())
        .await;

    // Assert
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json(),
        json!({"states": {
            hash(1).to_string(): "ready",
            hash(2).to_string(): "pending",
            hash(3).to_string(): "failed",
        }})
    );
    assert_eq!(
        stack.log(STATES),
        [format!(
            "{STATES} ({DEVICE_ACCOUNT}, [{:?}, {:?}, {:?}, {:?}])",
            hash(1),
            hash(2),
            hash(3),
            hash(4)
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_thousand_hashes_are_asked_and_a_thousand_and_one_are_422_batch_too_large() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        STATES,
        Ok::<HashMap<FileHash, ThumbState>, ThumbnailStateError>(HashMap::new()),
    );
    let many = |count: usize| {
        let hashes: Vec<String> = (0..count).map(|_| hash(9).to_string()).collect();
        json!({"content_hashes": hashes}).to_string()
    };

    // Act
    let exactly = stack
        .as_member("POST", "/api/v1/thumbnail-states", &many(1000))
        .await;
    let over = stack
        .as_member("POST", "/api/v1/thumbnail-states", &many(1001))
        .await;

    // Assert
    assert_eq!(exactly.status, 200);
    assert_eq!(over.status, 422);
    assert_eq!(over.code(), "batch_too_large");
    assert_eq!(stack.log(STATES).len(), 1);
    stack.stop().await;
}

#[tokio::test]
async fn a_body_that_is_not_a_list_of_hashes_never_reaches_08() {
    // Arrange
    let stack = Stack::start().await;
    let bodies = [
        "".to_owned(),
        "{}".to_owned(),
        r#"{"content_hashes":"x"}"#.to_owned(),
        r#"{"content_hashes":[5]}"#.to_owned(),
        r#"{"content_hashes":["abc"]}"#.to_owned(),
        r#"{"ids":[]}"#.to_owned(),
    ];

    // Act
    let mut replies = Vec::new();
    for body in &bodies {
        replies.push(
            stack
                .as_member("POST", "/api/v1/thumbnail-states", body)
                .await,
        );
    }
    let anonymous = stack
        .call(
            "POST",
            "/api/v1/thumbnail-states",
            &[],
            r#"{"content_hashes":[]}"#,
        )
        .await;

    // Assert
    for reply in &replies {
        assert_eq!(
            reply.status,
            400,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        assert_eq!(reply.code(), "invalid_request");
    }
    assert_eq!(anonymous.status, 401);
    assert_eq!(stack.log(STATES), Vec::<String>::new());
    stack.stop().await;
}
