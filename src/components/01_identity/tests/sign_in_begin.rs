//! AC-2 and the state half of AC-6 (IT-01-4): `begin` refuses bad input without writing anything, builds the
//! redirect URI from `public_url`, and stores only the hash of the state. Real PostgreSQL, the manual clock and a
//! real provider on a local port.

mod support;

use contracts::{ClientKind, SignInError};
use reqwest::Url;
use sc_error::ErrorCode;
use sc_ids::{RawToken, TokenKind};
use sunnycloud_identity::{IdentityError, IdentitySignIn};
use support::Harness;
use support::sign_in::{GITHUB, HOST, OIDC, begin_request, world};

const CALLBACK: &str = "https://sunnycloud.test/api/v1/auth/providers";

#[tokio::test]
async fn begin_with_an_unknown_provider_is_unknown_provider_and_writes_nothing() {
    // Arrange
    let mut world = world().await;

    // Act
    let result = world.service.begin(begin_request("nosuch", "/files")).await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::UnknownProvider)),
        "{result:?}"
    );
    assert_eq!(world.counts().await.oauth_states, 0);
    world.finish().await;
}

#[tokio::test]
async fn begin_with_a_hostile_return_path_is_invalid_return_path_for_each_and_writes_nothing() {
    // Arrange
    let mut world = world().await;
    let hostile = [
        "https://evil.test/",
        "//evil.test",
        "/\\evil.test",
        "/x\r\nSet-Cookie: a=b",
        "",
    ];

    for return_to in hostile {
        // Act
        let result = world.service.begin(begin_request(OIDC, return_to)).await;

        // Assert
        assert!(
            matches!(result, Err(SignInError::InvalidReturnPath)),
            "{return_to:?}: {result:?}"
        );
    }
    assert_eq!(world.counts().await.oauth_states, 0);
    world.finish().await;
}

#[tokio::test]
async fn begin_with_a_foreign_host_is_host_mismatch() {
    // Arrange
    let mut world = world().await;
    let mut request = begin_request(GITHUB, "/files");
    request.host = "evil.test".to_owned();

    // Act
    let result = world.service.begin(request).await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::HostMismatch)),
        "{result:?}"
    );
    assert_eq!(world.counts().await.oauth_states, 0);
    world.finish().await;
}

#[tokio::test]
async fn begin_for_a_mobile_client_without_a_code_challenge_is_an_invalid_request() {
    // Arrange
    let mut world = world().await;
    let mut request = begin_request(OIDC, "/files");
    request.client_kind = ClientKind::Android;

    // Act
    let result = world.service.begin(request).await;

    // Assert
    assert!(
        matches!(
            result,
            Err(SignInError::TrustedFailure {
                code: ErrorCode::InvalidRequest
            })
        ),
        "{result:?}"
    );
    assert_eq!(world.counts().await.oauth_states, 0);
    world.finish().await;
}

#[tokio::test]
async fn begin_for_a_mobile_client_with_a_code_challenge_stores_the_challenge() {
    // Arrange
    let mut world = world().await;
    let mut request = begin_request(OIDC, "/files");
    request.client_kind = ClientKind::Ios;
    request.code_challenge = Some("app-challenge".to_owned());

    // Act
    let result = world.service.begin(request).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    let stored: (String, Option<String>) =
        sqlx::query_as("SELECT client_kind, code_challenge FROM oauth_states")
            .fetch_one(&mut world.conn)
            .await
            .expect("state row");
    assert_eq!(stored, ("ios".to_owned(), Some("app-challenge".to_owned())));
    world.finish().await;
}

#[tokio::test]
async fn begin_redirects_to_each_providers_authorize_endpoint_with_a_redirect_uri_from_public_url()
{
    // Arrange
    let mut world = world().await;
    let base = world.idp.base_url();
    let cases = [
        (GITHUB, format!("{base}/login/oauth/authorize?")),
        (OIDC, format!("{base}/authorize?")),
    ];

    for (key, authorize_endpoint) in cases {
        // Act
        let (url, _) = world.begin(key).await;

        // Assert
        assert!(url.starts_with(&authorize_endpoint), "{key}: {url}");
        let query: std::collections::HashMap<_, _> = Url::parse(&url)
            .expect("url")
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(query["redirect_uri"], format!("{CALLBACK}/{key}/callback"));
        assert_eq!(query["code_challenge_method"], "S256");
    }
    assert_eq!(world.counts().await.oauth_states, 2);
    world.finish().await;
}

#[tokio::test]
async fn begin_stores_only_the_hash_of_state_and_binding_and_the_hash_of_the_invite() {
    // Arrange
    let mut world = world().await;
    let mut request = begin_request(OIDC, "/files");
    request.invite_token = Some(RawToken::from_presented(
        TokenKind::Invite,
        "sc_inv_plaintext-secret".to_owned(),
    ));

    // Act
    let redirect = world.service.begin(request).await.expect("begin");

    // Assert
    let state = Url::parse(&redirect.url)
        .expect("url")
        .query_pairs()
        .find(|(name, _)| name == "state")
        .map(|(_, value)| value.into_owned())
        .expect("state in the url");
    let binding = redirect.browser_binding.0;
    let (hash_matches, invite_matches, dump): (bool, bool, String) = sqlx::query_as(
        "SELECT state_hash = sha256(convert_to($1 || $2, 'UTF8')), \
         invite_token_hash = sha256(convert_to($3, 'UTF8')), row_to_json(o)::text \
         FROM oauth_states o",
    )
    .bind(&state)
    .bind(&binding)
    .bind("sc_inv_plaintext-secret")
    .fetch_one(&mut world.conn)
    .await
    .expect("state row");
    assert!(hash_matches, "state_hash is SHA-256(state || binding)");
    assert!(invite_matches, "invite_token_hash is SHA-256(invite token)");
    for secret in [state.as_str(), binding.as_str(), "sc_inv_plaintext-secret"] {
        assert!(!dump.contains(secret), "plaintext secret in oauth_states");
    }
    world.finish().await;
}

#[tokio::test]
async fn service_construction_fails_for_an_http_public_url_without_the_development_flag() {
    // Arrange
    let mut harness = Harness::new().await;
    harness.config.public_url = format!("http://{HOST}");
    harness.config.allow_plain_http_dev = false;

    // Act
    let result = IdentitySignIn::new(
        harness.api.clone(),
        harness.clock.clone(),
        &harness.config,
        harness.audit(),
        support::sign_in::roomy_limiter(&harness),
    );

    // Assert
    assert!(matches!(result, Err(IdentityError::Config(_))));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
