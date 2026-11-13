use contracts::{DerivedBlob, DerivedError, DownloadError, PublicShareError, ShareDerived};

use super::support::*;
use crate::content::support::KIB;

fn thumbnail(tail: &str) -> String {
    link(&format!("/thumbnail{tail}"))
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
async fn a_share_thumbnail_asks_06_then_08_then_03_once_each_and_never_asks_08_to_authorise() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/webp", KIB)));

    // Act
    let reply = stack.as_visitor("GET", &thumbnail(""), &[]).await;

    // Assert
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("content-type"), Some("image/webp"));
    assert_eq!(reply.body.len(), KIB);
    assert_eq!(
        stack.order(),
        [AUTHORIZE_DERIVED, OPEN_DERIVED_FOR, OPEN_BLOB]
    );
    assert!(
        stack
            .log(OPEN_DERIVED)
            .iter()
            .all(|line| line.starts_with(OPEN_DERIVED_FOR))
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_file_and_account_of_06_reach_08_and_the_blob_and_stored_type_of_08_reach_03() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Ok::<DerivedBlob, DerivedError>(derived("image/jpeg")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/webp", KIB)));

    // Act
    stack.as_visitor("GET", &thumbnail(""), &[]).await;

    // Assert
    let file = format!("{:?}", shared_file().expect("a file").file);
    let to_08 = stack.log(OPEN_DERIVED_FOR);
    assert!(to_08[0].contains(&file), "{}", to_08[0]);
    assert!(to_08[0].contains("Thumb"), "{}", to_08[0]);
    assert!(
        to_08[0].contains(&format!("({SHARE_ACCOUNT}, ")),
        "{}",
        to_08[0]
    );
    let to_03 = stack.log(OPEN_BLOB);
    assert!(
        to_03[0].contains(&format!("{SHARE_ACCOUNT}")),
        "{}",
        to_03[0]
    );
    assert!(to_03[0].contains(&format!("{:?}", blob())), "{}", to_03[0]);
    assert!(to_03[0].contains("Some(\"image/jpeg\")"), "{}", to_03[0]);
    stack.stop().await;
}

#[tokio::test]
async fn size_picks_the_derived_kind_and_an_unknown_size_is_refused_before_any_port_call() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/webp", KIB)));

    // Act
    let preview = stack
        .as_visitor("GET", &thumbnail("?size=preview"), &[])
        .await;
    let calls_after_preview = stack.order().len();
    let large = stack
        .as_visitor("GET", &thumbnail("?size=large"), &[])
        .await;

    // Assert
    assert_eq!(preview.status, 200);
    assert!(stack.log(OPEN_DERIVED_FOR)[0].contains("Preview"));
    assert_eq!(large.status, 400);
    assert_eq!(stack.order().len(), calls_after_preview);
    stack.stop().await;
}

#[tokio::test]
async fn a_head_answers_the_headers_of_the_same_calls_with_no_body() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/webp", KIB)));

    // Act
    let reply = stack.as_visitor("HEAD", &thumbnail(""), &[]).await;

    // Assert
    assert_eq!(reply.status, 200);
    assert!(reply.body.is_empty());
    assert_eq!(reply.header("content-type"), Some("image/webp"));
    stack.stop().await;
}

#[tokio::test]
async fn the_thumbnail_carries_no_cross_origin_header() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack
        .script
        .push(OPEN_BLOB, served(full("image/webp", KIB)));

    // Act
    let reply = stack
        .as_visitor(
            "GET",
            &thumbnail(""),
            &[("origin", "https://elsewhere.example")],
        )
        .await;

    // Assert
    assert_eq!(cors_headers(&reply), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn a_missing_link_a_locked_link_and_a_missing_thumbnail_answer_their_own_errors_without_calling_03()
 {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        AUTHORIZE_DERIVED,
        Err::<ShareDerived, PublicShareError>(PublicShareError::NotFound),
    );
    stack.script.push(
        AUTHORIZE_DERIVED,
        Err::<ShareDerived, PublicShareError>(PublicShareError::PasswordRequired),
    );
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Err::<DerivedBlob, DerivedError>(DerivedError::NotReady),
    );

    // Act
    let missing = stack.as_visitor("GET", &thumbnail(""), &[]).await;
    let locked = stack.as_visitor("GET", &thumbnail(""), &[]).await;
    let not_ready = stack.as_visitor("GET", &thumbnail(""), &[]).await;

    // Assert
    assert_eq!(
        (missing.status, missing.code().as_str()),
        (404, "not_found")
    );
    assert_eq!(
        (locked.status, locked.code().as_str()),
        (401, "password_required")
    );
    assert_eq!(
        (not_ready.status, not_ready.code().as_str()),
        (404, "thumbnail_not_ready")
    );
    assert!(stack.log(OPEN_BLOB).is_empty());
    stack.stop().await;
}

#[tokio::test]
async fn a_failed_03_answer_is_the_content_error_not_a_clean_empty_thumbnail() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(AUTHORIZE_DERIVED, shared_file());
    stack.script.push(
        OPEN_DERIVED_FOR,
        Ok::<DerivedBlob, DerivedError>(derived("image/webp")),
    );
    stack.script.push(
        OPEN_BLOB,
        Err::<contracts::Download, DownloadError>(DownloadError::ContentUnavailable),
    );

    // Act
    let reply = stack.as_visitor("GET", &thumbnail(""), &[]).await;

    // Assert
    assert_eq!(reply.status, 404);
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    stack.stop().await;
}
