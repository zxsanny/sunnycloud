use contracts::{AccessGrant, PublicShareError, ShareContent, ShareOpen};

use super::support::*;

const LINK_STATES: [&str; 4] = ["unknown", "expired", "revoked", "deleted"];

#[tokio::test]
async fn every_missing_expired_revoked_or_deleted_link_answers_the_same_404() {
    // Arrange
    let stack = Stack::start().await;
    for _ in LINK_STATES {
        stack
            .script
            .push(OPEN, Ok::<ShareOpen, PublicShareError>(ShareOpen::NotFound));
        stack.script.push(
            AUTHORIZE_CONTENT,
            Err::<ShareContent, PublicShareError>(PublicShareError::NotFound),
        );
    }
    stack.script.push(
        VERIFY,
        Err::<AccessGrant, PublicShareError>(PublicShareError::NotFound),
    );
    stack.script.push(
        AUTHORIZE_CONTENT,
        Err::<ShareContent, PublicShareError>(PublicShareError::NotFound),
    );

    // Act
    let mut answers = Vec::new();
    for _ in LINK_STATES {
        answers.push(stack.as_visitor("GET", &link(""), &[]).await);
        answers.push(stack.as_visitor("GET", &link("/content"), &[]).await);
    }
    answers.push(
        stack
            .call(
                "POST",
                &link("/password-verifications"),
                &[],
                r#"{"password":"guess"}"#,
            )
            .await,
    );
    answers.push(stack.as_visitor("HEAD", &link("/content"), &[]).await);

    // Assert
    assert_eq!(answers.len(), 10);
    let first = shape(&answers[0]);
    for answer in &answers {
        assert_eq!(answer.status, 404);
    }
    let with_body: Vec<_> = answers
        .iter()
        .filter(|answer| !answer.body.is_empty())
        .collect();
    assert_eq!(with_body.len(), 9);
    for answer in with_body {
        assert_eq!(answer.code(), "not_found");
        assert_eq!(shape(answer), first);
    }
    let ids: Vec<&str> = answers
        .iter()
        .filter_map(|answer| answer.header("x-request-id"))
        .collect();
    assert_eq!(ids.len(), 10);
    stack.stop().await;
}

#[tokio::test]
async fn a_token_that_is_not_text_is_the_same_404_and_never_reaches_06() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(OPEN, Ok::<ShareOpen, PublicShareError>(ShareOpen::NotFound));
    let unknown = stack.as_visitor("GET", &link(""), &[]).await;

    // Act
    let binary = stack
        .as_visitor("GET", "/api/v1/public-shares/%FF%FE", &[])
        .await;

    // Assert
    assert_eq!(binary.status, 404);
    assert_eq!(shape(&binary), shape(&unknown));
    assert_eq!(stack.log("PublicShareService").len(), 1);
    stack.stop().await;
}

#[tokio::test]
async fn an_open_link_names_only_the_file_and_the_end_of_the_link() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN,
        Ok::<ShareOpen, PublicShareError>(ShareOpen::Available {
            name: "holiday.png".to_owned(),
            size: 2048,
            media_type: Some("image/png".to_owned()),
            expires_at: sc_clock::Timestamp::from_unix_micros(1_793_956_272_123_456),
        }),
    );

    // Act
    let reply = stack.as_visitor("GET", &link(""), &[]).await;

    // Assert
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json(),
        serde_json::json!({
            "name": "holiday.png",
            "size": 2048,
            "media_type": "image/png",
            "expires_at": "2026-11-06T09:11:12.123456Z",
        })
    );
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert_eq!(stack.log(OPEN), [format!("{OPEN} {TOKEN} 127.0.0.1")]);
    stack.stop().await;
}

#[tokio::test]
async fn too_many_attempts_is_429_with_the_wait_the_port_reports() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN,
        Err::<ShareOpen, PublicShareError>(PublicShareError::RateLimited {
            retry_after: std::time::Duration::from_secs(17),
        }),
    );

    // Act
    let reply = stack.as_visitor("GET", &link(""), &[]).await;

    // Assert
    assert_eq!(reply.status, 429);
    assert_eq!(reply.header("retry-after"), Some("17"));
    assert_eq!(reply.code(), "rate_limited");
    stack.stop().await;
}
