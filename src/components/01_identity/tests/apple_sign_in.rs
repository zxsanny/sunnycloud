//! AZ-4024 AC-1, AC-2 and AC-5 (FT-P-63, NFT-SEC-38): Sign in with Apple through the same `begin` and `complete`
//! as the other kinds, the client secret the code exchange carries, and the provider list. Real PostgreSQL, the
//! manual clock and the in-process provider answering as Apple does; the team key is generated at run time.

mod support;

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use contracts::{Authenticator, ClientKind, Credential, ProviderInfo, SignInError, SignInOutcome};
use reqwest::Url;
use serde_json::Value;
use support::apple::{APPLE, SERVICES_ID, TEAM_ID, TeamKey, apple_world};
use support::audit;
use support::auth::authenticator;
use support::logs::LogCapture;
use support::sign_in::{CHALLENGE, CLIENT_IP, HOST, World, begin_request, params};

const SUBJECT: &str = "apple-sub-1";
const CALLBACK: &str = "https://sunnycloud.test/api/v1/auth/providers/apple/callback";

fn query_of(url: &str) -> HashMap<String, String> {
    Url::parse(url)
        .expect("authorization url")
        .query_pairs()
        .into_owned()
        .collect()
}

fn claims_of(secret: &str) -> Value {
    let payload = secret.split('.').nth(1).expect("payload segment");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).expect("base64url")).expect("claims")
}

/// A web sign-in of `SUBJECT`: the authorization URL and the outcome.
async fn web_sign_in(world: &World) -> (String, Result<SignInOutcome, SignInError>) {
    let (url, binding) = world.begin(APPLE).await;
    let callback = world.idp.authorize(&url, SUBJECT);
    let outcome = world
        .service
        .complete(APPLE, params(callback), &binding, HOST, CLIENT_IP)
        .await;
    (url, outcome)
}

#[tokio::test]
async fn a_web_sign_in_through_apple_gives_the_identitys_account_a_session_and_the_url_has_apples_shape()
 {
    // Arrange
    let team = TeamKey::generate();
    let mut world = apple_world(&team, |_| {}).await;
    let account = world.known_identity(APPLE, SUBJECT).await;

    // Act
    let (url, outcome) = web_sign_in(&world).await;

    // Assert
    let query = query_of(&url);
    assert!(
        url.starts_with(&format!("{}/", world.idp.apple_issuer())),
        "{url}"
    );
    assert_eq!(
        query.get("client_id").map(String::as_str),
        Some(SERVICES_ID)
    );
    assert_eq!(
        query.get("redirect_uri").map(String::as_str),
        Some(CALLBACK)
    );
    assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(
        query.get("response_mode").map(String::as_str),
        Some("query")
    );
    assert!(
        query.contains_key("state") && query.contains_key("nonce"),
        "{url}"
    );
    assert!(!query.contains_key("scope"), "{url}");
    assert!(!query.contains_key("code_challenge"), "{url}");
    let Ok(SignInOutcome::Session { tokens, return_to }) = outcome else {
        panic!("expected a session, got {outcome:?}");
    };
    let principal = authenticator(&world.harness)
        .authenticate(Credential::Session(tokens.access))
        .await
        .expect("the access token authenticates");
    assert_eq!(
        (principal.account_id, return_to),
        (account, "/files".to_owned())
    );
    world.finish().await;
}

#[tokio::test]
async fn a_mobile_sign_in_through_apple_ends_in_the_one_time_code_and_sends_apple_no_challenge() {
    // Arrange
    let team = TeamKey::generate();
    let mut world = apple_world(&team, |_| {}).await;
    let account = world.known_identity(APPLE, SUBJECT).await;
    let mut request = begin_request(APPLE, "/files");
    request.client_kind = ClientKind::Ios;
    request.code_challenge = Some(CHALLENGE.to_owned());
    let redirect = world.service.begin(request).await.expect("begin");
    let callback = world.idp.authorize(&redirect.url, SUBJECT);

    // Act
    let outcome = world
        .service
        .complete(
            APPLE,
            params(callback),
            &redirect.browser_binding,
            HOST,
            CLIENT_IP,
        )
        .await;

    // Assert
    let query = query_of(&redirect.url);
    assert!(!query.contains_key("code_challenge") && !query.contains_key("scope"));
    assert!(
        matches!(outcome, Ok(SignInOutcome::MobileCode { .. })),
        "{outcome:?}"
    );
    let (owner, challenge, kind): (uuid::Uuid, String, String) =
        sqlx::query_as("SELECT account_id, code_challenge, client_kind FROM login_codes")
            .fetch_one(&mut world.conn)
            .await
            .expect("one login code");
    assert_eq!(
        (owner, challenge.as_str(), kind.as_str()),
        (account, CHALLENGE, "ios")
    );
    assert_eq!(world.counts().await.sessions, 0);
    world.finish().await;
}

#[tokio::test]
async fn the_exchange_is_a_client_secret_post_with_an_es256_jwt_and_each_exchange_after_the_renewal_point_has_a_new_one()
 {
    // Arrange
    let team = TeamKey::generate();
    let mut world = apple_world(&team, |_| {}).await;
    world.known_identity(APPLE, SUBJECT).await;

    // Act
    let first = web_sign_in(&world).await.1;
    world.harness.clock.tick(Duration::from_secs(6 * 60));
    let second = web_sign_in(&world).await.1;

    // Assert
    assert!(
        matches!(first, Ok(SignInOutcome::Session { .. })),
        "{first:?}"
    );
    assert!(
        matches!(second, Ok(SignInOutcome::Session { .. })),
        "{second:?}"
    );
    let secrets = world.idp.client_secrets();
    assert_eq!(
        secrets.len(),
        2,
        "the mock validated both against the team key"
    );
    let [one, two] = [claims_of(&secrets[0]), claims_of(&secrets[1])];
    assert_ne!(secrets[0], secrets[1]);
    assert_eq!(one["iss"], TEAM_ID);
    assert_eq!(one["sub"], SERVICES_ID);
    assert!(two["iat"].as_i64() > one["exp"].as_i64(), "{one} {two}");
    world.finish().await;
}

#[tokio::test]
async fn neither_the_key_file_nor_a_client_secret_appears_in_a_log_an_audit_row_or_an_error() {
    // Arrange
    let team = TeamKey::generate();
    let logs = LogCapture::install();
    let mut world = apple_world(&team, |_| {}).await;
    world.known_identity(APPLE, SUBJECT).await;
    let accepted = web_sign_in(&world).await.1;
    let stranger_team = TeamKey::generate();
    world
        .idp
        .check_client_secrets(stranger_team.check(world.harness.clock.clone()));

    // Act
    let refused = web_sign_in(&world).await.1;

    // Assert
    assert!(
        matches!(accepted, Ok(SignInOutcome::Session { .. })),
        "{accepted:?}"
    );
    assert!(
        matches!(refused, Err(SignInError::TrustedFailure { .. })),
        "{refused:?}"
    );
    let key_body: String = team
        .p8
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let mut needles = world.idp.client_secrets();
    needles.push(key_body);
    let mut haystack: Vec<String> = logs.lines().into_iter().map(|line| line.text).collect();
    haystack.push(format!("{refused:?} {refused:#?}"));
    haystack.extend(
        audit::rows(&mut world.conn)
            .await
            .into_iter()
            .map(|row| format!("{row:?}")),
    );
    for text in &haystack {
        for needle in &needles {
            assert!(!text.contains(needle.as_str()), "a secret reached: {text}");
        }
    }
    assert!(!haystack.is_empty());
    world.finish().await;
}

#[tokio::test]
async fn the_provider_list_names_apple_by_kind_in_configured_order_next_to_github() {
    // Arrange
    let team = TeamKey::generate();
    let world = apple_world(&team, |_| {}).await;

    // Act
    let list = world.service.list_providers().await.expect("list");

    // Assert
    let info = |key: &str, display_name: &str, kind: &str| ProviderInfo {
        key: key.to_owned(),
        display_name: display_name.to_owned(),
        kind: kind.to_owned(),
    };
    assert_eq!(
        list.providers,
        vec![
            info(APPLE, "Apple", "apple"),
            info("gh", "GitHub", "github")
        ]
    );
    world.finish().await;
}
