//! AC-3 (IT-01-5 cases 1 to 11, FT-N-23): every negative callback ends in its documented error and creates no
//! session, account, identity or login code. Case 12 (wrong code verifier) is in `mobile_code.rs`.
//! A known identity exists in every test, so a failure is caused by the mutation and not by a missing identity.

mod support;

use std::time::Duration;

use contracts::{BrowserBinding, SignInError, SignInOutcome};
use sc_error::ErrorCode;
use support::sign_in::{Counts, GITHUB, HOST, OIDC, World, begin_request, params, world};

const SUBJECT: &str = "sub-1";

/// Rows a failed callback must not create.
fn created(counts: &Counts) -> [i64; 4] {
    [
        counts.sessions,
        counts.accounts,
        counts.identities,
        counts.login_codes,
    ]
}

async fn world_with_known_identity() -> World {
    let mut world = world().await;
    world.known_identity(OIDC, SUBJECT).await;
    world
}

/// `begin` plus the provider login: the callback parameters and the binding cookie.
async fn authorized(world: &World) -> (Vec<(String, String)>, BrowserBinding) {
    let (url, binding) = world.begin(OIDC).await;
    (world.idp.authorize(&url, SUBJECT), binding)
}

fn is_trusted_failure(result: &Result<SignInOutcome, SignInError>) -> bool {
    matches!(
        result,
        Err(SignInError::TrustedFailure {
            code: ErrorCode::Unauthenticated
        })
    )
}

async fn complete(
    world: &World,
    provider: &str,
    callback: Vec<(String, String)>,
    binding: &BrowserBinding,
    host: &str,
) -> Result<SignInOutcome, SignInError> {
    world
        .service
        .complete(provider, params(callback), binding, host)
        .await
}

async fn trusted_failure_for(change: impl FnOnce(&mut support::provider::Grant) + Send + 'static) {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (url, binding) = world.begin(OIDC).await;
    let callback = world.idp.authorize_with(&url, SUBJECT, change);
    let before = world.counts().await;

    // Act
    let result = complete(&world, OIDC, callback, &binding, HOST).await;

    // Assert
    assert!(is_trusted_failure(&result), "{result:?}");
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}

#[tokio::test]
async fn case_1_missing_state_is_untrusted_state_and_consumes_no_row() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let without_state = vec![callback[0].clone()];
    let before = world.counts().await;

    // Act
    let result = complete(&world, OIDC, without_state, &binding, HOST).await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::UntrustedState)),
        "{result:?}"
    );
    assert_eq!(world.counts().await, before);
    world.finish().await;
}

#[tokio::test]
async fn case_2_replayed_state_is_untrusted_state_the_second_time() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let first = complete(&world, OIDC, callback.clone(), &binding, HOST).await;
    assert!(
        matches!(first, Ok(SignInOutcome::Session { .. })),
        "{first:?}"
    );
    let before = world.counts().await;

    // Act
    let replay = complete(&world, OIDC, callback, &binding, HOST).await;

    // Assert
    assert!(
        matches!(replay, Err(SignInError::UntrustedState)),
        "{replay:?}"
    );
    assert_eq!(world.counts().await, before);
    world.finish().await;
}

#[tokio::test]
async fn case_3_foreign_state_is_untrusted_state_and_leaves_the_real_state_usable() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let before = world.counts().await;

    // Act
    let foreign = complete(
        &world,
        OIDC,
        callback.clone(),
        &BrowserBinding("another-browser".to_owned()),
        HOST,
    )
    .await;

    // Assert
    assert!(
        matches!(foreign, Err(SignInError::UntrustedState)),
        "{foreign:?}"
    );
    assert_eq!(world.counts().await, before);
    let genuine = complete(&world, OIDC, callback, &binding, HOST).await;
    assert!(
        matches!(genuine, Ok(SignInOutcome::Session { .. })),
        "{genuine:?}"
    );
    world.finish().await;
}

#[tokio::test]
async fn case_4_expired_state_is_untrusted_state() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let ttl = world.harness.config.oauth_state_ttl;
    world.harness.clock.tick(ttl + Duration::from_millis(1));
    let before = world.counts().await;

    // Act
    let result = complete(&world, OIDC, callback, &binding, HOST).await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::UntrustedState)),
        "{result:?}"
    );
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}

#[tokio::test]
async fn case_5_wrong_provider_key_is_untrusted_state() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let before = world.counts().await;

    // Act
    let result = complete(&world, GITHUB, callback, &binding, HOST).await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::UntrustedState)),
        "{result:?}"
    );
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}

#[tokio::test]
async fn case_6_wrong_nonce_in_the_id_token_is_a_trusted_failure() {
    trusted_failure_for(|grant| grant.nonce = "forged-nonce".to_owned()).await;
}

#[tokio::test]
async fn case_7_wrong_audience_is_a_trusted_failure() {
    trusted_failure_for(|grant| grant.audience = "another-client".to_owned()).await;
}

#[tokio::test]
async fn case_8_wrong_issuer_is_a_trusted_failure() {
    trusted_failure_for(|grant| grant.issuer = "https://evil.test".to_owned()).await;
}

#[tokio::test]
async fn case_9_absolute_return_to_at_begin_is_invalid_return_path_and_stores_nothing() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let before = world.counts().await;

    // Act
    let result = world
        .service
        .begin(begin_request(OIDC, "https://evil.test/after"))
        .await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::InvalidReturnPath)),
        "{result:?}"
    );
    assert_eq!(world.counts().await, before);
    world.finish().await;
}

#[tokio::test]
async fn case_10_foreign_host_at_complete_is_host_mismatch_and_consumes_no_state() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let before = world.counts().await;

    // Act
    let result = complete(&world, OIDC, callback, &binding, "evil.test").await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::HostMismatch)),
        "{result:?}"
    );
    assert_eq!(world.counts().await, before);
    world.finish().await;
}

#[tokio::test]
async fn case_11_reused_one_time_provider_code_is_a_trusted_failure() {
    // Arrange
    let mut world = world_with_known_identity().await;
    let (callback, binding) = authorized(&world).await;
    let first = complete(&world, OIDC, callback.clone(), &binding, HOST).await;
    assert!(
        matches!(first, Ok(SignInOutcome::Session { .. })),
        "{first:?}"
    );
    let (second_url, second_binding) = world.begin(OIDC).await;
    let second_state = world.idp.authorize(&second_url, SUBJECT)[1].clone();
    let reused = vec![callback[0].clone(), second_state];
    let before = world.counts().await;

    // Act
    let result = complete(&world, OIDC, reused, &second_binding, HOST).await;

    // Assert
    assert!(is_trusted_failure(&result), "{result:?}");
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}
