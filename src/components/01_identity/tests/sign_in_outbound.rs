//! AC-4 and AC-5 (NFT-RES-24, NFT-SEC-30 component part): a stalling provider leaves nothing behind within the
//! configured timeout, and provider calls reach only what the rules allow. Real PostgreSQL, the manual clock
//! and real providers on local ports (one of them the canary that must receive nothing).

mod support;

use std::time::{Duration, Instant};

use contracts::{Authenticator, SignInError};
use support::auth::{FAR_FUTURE, authenticator, session};
use support::provider::{Behaviour, FakeProvider};
use support::sign_in::{GITHUB, HOST, OIDC, World, params, world_with};
use support::{START, fixtures};
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_millis(300);

async fn stalled_provider_leaves_nothing_behind(provider: &str, subject: &str) {
    // Arrange
    let mut world = world_with(|config| config.provider_timeout = TIMEOUT).await;
    world.known_identity(provider, subject).await;
    let bystander = Uuid::new_v4();
    let at = START.as_unix_micros();
    fixtures::account(&mut world.conn, bystander, at).await;
    fixtures::live_session(
        &mut world.conn,
        bystander,
        "bystander-token",
        "other",
        FAR_FUTURE,
        at,
    )
    .await;
    let (url, binding) = world.begin(provider).await;
    let callback = world.idp.authorize(&url, subject);
    world
        .idp
        .behave(|behaviour| behaviour.stall_token_endpoint = true);
    let before = world.counts().await;

    // Act
    let started = Instant::now();
    let result = world
        .service
        .complete(provider, params(callback), &binding, HOST)
        .await;
    let elapsed = started.elapsed();

    // Assert
    assert!(
        matches!(result, Err(SignInError::ProviderUnavailable)),
        "{result:?}"
    );
    assert!(
        elapsed < TIMEOUT + Duration::from_secs(1),
        "took {elapsed:?}"
    );
    let mut after = world.counts().await;
    assert_eq!(after.oauth_states, 0, "the state is spent");
    after.oauth_states = before.oauth_states;
    assert_eq!(after, before);
    let principal = authenticator(&world.harness)
        .authenticate(session("bystander-token"))
        .await
        .expect("the existing session still authenticates");
    assert_eq!(principal.account_id, bystander);
    world.finish().await;
}

#[tokio::test]
async fn a_stalling_oidc_provider_gives_provider_unavailable_within_the_timeout_and_nothing_changes()
 {
    stalled_provider_leaves_nothing_behind(OIDC, "sub-stall").await;
}

#[tokio::test]
async fn a_stalling_github_provider_gives_provider_unavailable_within_the_timeout_and_nothing_changes()
 {
    stalled_provider_leaves_nothing_behind(GITHUB, "4242").await;
}

fn hostile_oidc_endpoints() -> Vec<(&'static str, Behaviour)> {
    let token = |url: &str| Behaviour {
        advertised_token_endpoint: Some(url.to_owned()),
        ..Behaviour::default()
    };
    vec![
        (
            "token endpoint at the metadata address",
            token("http://169.254.169.254/token"),
        ),
        (
            "token endpoint at an IPv6 link-local address",
            token("http://[fe80::1]/token"),
        ),
        (
            "token endpoint at the metadata host name",
            token("http://metadata.google.internal/token"),
        ),
        (
            "token endpoint with the file scheme",
            token("file:///etc/passwd"),
        ),
        (
            "key set at the metadata address",
            Behaviour {
                advertised_jwks_uri: Some("http://169.254.169.254/jwks".to_owned()),
                ..Behaviour::default()
            },
        ),
    ]
}

/// One sign-in attempt with `behaviour` applied between `begin` and `complete`; the outcome and how long it took.
async fn attempt(
    world: &World,
    provider: &str,
    behaviour: Behaviour,
) -> (Result<(), SignInError>, Duration) {
    let (url, binding) = world.begin(provider).await;
    let callback = world
        .idp
        .authorize(&url, if provider == OIDC { "sub-1" } else { "4242" });
    world.idp.behave(|current| *current = behaviour);
    let started = Instant::now();
    let result = world
        .service
        .complete(provider, params(callback), &binding, HOST)
        .await;
    (result.map(drop), started.elapsed())
}

#[tokio::test]
async fn an_issuer_that_advertises_refused_destinations_fails_the_sign_in_without_a_session() {
    // Arrange
    let mut world = world_with(|config| config.provider_timeout = Duration::from_secs(3)).await;
    world.known_identity(OIDC, "sub-1").await;

    for (name, behaviour) in hostile_oidc_endpoints() {
        // Act
        let (result, elapsed) = attempt(&world, OIDC, behaviour).await;

        // Assert
        assert!(
            matches!(
                result,
                Err(SignInError::ProviderUnavailable | SignInError::TrustedFailure { .. })
            ),
            "{name}: {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "{name}: refused only after {elapsed:?}"
        );
    }
    assert_eq!(world.counts().await.sessions, 0);
    world.finish().await;
}

#[tokio::test]
async fn a_redirect_from_a_provider_endpoint_is_not_followed() {
    // Arrange
    let canary = FakeProvider::start(0).await;
    let mut world = world_with(|_| {}).await;
    world.known_identity(OIDC, "sub-1").await;
    world.known_identity(GITHUB, "4242").await;
    let redirect = Behaviour {
        token_redirect: Some(format!("{}/canary", canary.base_url())),
        ..Behaviour::default()
    };

    for provider in [OIDC, GITHUB] {
        // Act
        let (result, _) = attempt(&world, provider, redirect.clone()).await;

        // Assert
        assert!(
            matches!(result, Err(SignInError::ProviderUnavailable)),
            "{provider}: {result:?}"
        );
    }
    assert_eq!(canary.hits(""), 0, "the canary received a request");
    assert_eq!(world.counts().await.sessions, 0);
    world.finish().await;
}

#[tokio::test]
async fn an_issuer_on_a_non_public_local_address_still_signs_in() {
    // Arrange
    let mut world = world_with(|_| {}).await;
    world.known_identity(OIDC, "sub-1").await;
    assert!(world.idp.address().ip().is_loopback());

    // Act
    let (result, _) = attempt(&world, OIDC, Behaviour::default()).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(world.counts().await.sessions, 1);
    world.finish().await;
}
