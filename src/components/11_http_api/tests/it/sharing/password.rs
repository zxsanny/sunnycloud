use contracts::{AccessGrant, PublicShareError, ShareContent, ShareOpen};

use super::support::*;

const PASSWORD: &str = "s3cret pass";

#[tokio::test]
async fn a_password_link_reveals_nothing_before_unlock_and_the_grant_is_a_scoped_cookie_after() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        OPEN,
        Ok::<ShareOpen, PublicShareError>(ShareOpen::PasswordRequired),
    );
    stack.script.push(
        AUTHORIZE_CONTENT,
        Err::<ShareContent, PublicShareError>(PublicShareError::PasswordRequired),
    );
    let granted = grant(600);
    stack
        .script
        .push(VERIFY, Ok::<AccessGrant, PublicShareError>(granted.clone()));
    stack.script.push(AUTHORIZE_CONTENT, allowed());
    stack
        .script
        .push(OPEN_REVISION, served(full("image/png", 512)));

    // Act
    let opened = stack.as_visitor("GET", &link(""), &[]).await;
    let locked = stack.as_visitor("GET", &link("/content"), &[]).await;
    let verified = stack
        .call(
            "POST",
            &link("/password-verifications"),
            &[],
            &serde_json::json!({"password": PASSWORD}).to_string(),
        )
        .await;
    let cookie = verified
        .header("set-cookie")
        .expect("the verification sets the grant")
        .to_owned();
    let pair = cookie
        .split(';')
        .next()
        .expect("a name=value pair")
        .to_owned();
    let unlocked = stack
        .as_visitor("GET", &link("/content"), &[("cookie", pair.as_str())])
        .await;

    // Assert
    for answer in [&opened, &locked] {
        assert_eq!(answer.status, 401);
        assert_eq!(answer.code(), "password_required");
        let text = String::from_utf8_lossy(&answer.body).into_owned();
        for field in ["name", "size", "media_type", "expires_at"] {
            assert!(!text.contains(&format!("\"{field}\"")), "{text}");
        }
    }
    assert_eq!(verified.status, 204);
    assert!(verified.body.is_empty());
    let attributes: Vec<&str> = cookie.split("; ").collect();
    assert!(attributes.contains(&"HttpOnly"), "{cookie}");
    assert!(attributes.contains(&"Secure"), "{cookie}");
    assert!(attributes.contains(&"SameSite=Lax"), "{cookie}");
    let path = format!("Path=/api/v1/public-shares/{TOKEN}");
    assert!(attributes.contains(&path.as_str()), "{cookie}");
    assert!(
        !attributes.iter().any(|a| a.starts_with("Domain")),
        "{cookie}"
    );
    let max_age: u64 = attributes
        .iter()
        .find_map(|a| a.strip_prefix("Max-Age="))
        .and_then(|age| age.parse().ok())
        .expect("a max age");
    assert!((590..=600).contains(&max_age), "{cookie}");
    assert_eq!(unlocked.status, 200);
    let calls = stack.log(AUTHORIZE_CONTENT);
    assert!(calls[0].contains("no grant"), "{calls:?}");
    assert!(
        calls[1].contains(&format!(
            "grant={} expires_at={}",
            granted.value,
            granted.expires_at.as_unix_micros()
        )),
        "{calls:?}"
    );
    assert_eq!(
        stack.log(VERIFY),
        [format!("{VERIFY} {TOKEN} {PASSWORD} 127.0.0.1")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_wrong_password_is_401_password_required_and_sets_no_cookie() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        VERIFY,
        Err::<AccessGrant, PublicShareError>(PublicShareError::WrongPassword),
    );

    // Act
    let reply = stack
        .call(
            "POST",
            &link("/password-verifications"),
            &[],
            r#"{"password":"nope"}"#,
        )
        .await;

    // Assert
    assert_eq!(reply.status, 401);
    assert_eq!(reply.code(), "password_required");
    assert_eq!(reply.header("set-cookie"), None);
    stack.stop().await;
}

#[tokio::test]
async fn a_body_that_is_not_a_password_never_reaches_06() {
    // Arrange
    let stack = Stack::start().await;
    let bodies = [
        "",
        "not json",
        "{}",
        r#"{"password":5}"#,
        r#"{"password":"a","x":1}"#,
    ];

    // Act
    let mut replies = Vec::new();
    for body in bodies {
        replies.push(
            stack
                .call("POST", &link("/password-verifications"), &[], body)
                .await,
        );
    }

    // Assert
    for reply in &replies {
        assert_eq!(reply.status, 400);
        assert_eq!(reply.code(), "invalid_request");
    }
    assert_eq!(stack.log("PublicShareService"), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn a_grant_cookie_that_is_malformed_is_no_grant() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        AUTHORIZE_CONTENT,
        Err::<ShareContent, PublicShareError>(PublicShareError::PasswordRequired),
    );

    // Act
    let reply = stack
        .as_visitor(
            "GET",
            &link("/content"),
            &[("cookie", "__Secure-share-grant=garbage")],
        )
        .await;

    // Assert
    assert_eq!(reply.status, 401);
    assert!(stack.log(AUTHORIZE_CONTENT)[0].contains("no grant"));
    stack.stop().await;
}

#[tokio::test]
async fn a_password_over_the_auth_limit_is_413_before_the_body_is_read() {
    // Arrange
    let stack = Stack::start().await;
    let body = serde_json::json!({"password": "x".repeat(17 * 1024)}).to_string();

    // Act
    let reply = stack
        .call("POST", &link("/password-verifications"), &[], &body)
        .await;

    // Assert
    assert_eq!(reply.status, 413);
    assert_eq!(reply.code(), "size_too_large");
    assert_eq!(stack.log("PublicShareService"), Vec::<String>::new());
    stack.stop().await;
}
