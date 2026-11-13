//! AC-1 and AC-6 (FT-P-62, ST-01-5): a known identity signs in through each provider kind and reaches its own
//! account whatever email the provider reports; an unknown identity creates nothing; no secret is stored in
//! the clear. Real PostgreSQL, the manual clock and a real provider on a local port.

mod support;

use contracts::{Authenticator, Credential, SignInError, SignInOutcome};
use sc_error::ErrorCode;
use support::auth::authenticator;
use support::sign_in::{GITHUB, OIDC, World, params, world};
use uuid::Uuid;

/// `begin`, the provider login of `subject` (with `email` for OIDC) and `complete`.
async fn sign_in(
    world: &World,
    provider: &str,
    subject: &str,
    email: &str,
) -> Result<SignInOutcome, SignInError> {
    let (url, binding) = world.begin(provider).await;
    let address = email.to_owned();
    let callback = world.idp.authorize_with(&url, subject, move |grant| {
        grant.email = Some((address, true))
    });
    world
        .service
        .complete(
            provider,
            params(callback),
            &binding,
            support::sign_in::HOST,
            support::sign_in::CLIENT_IP,
        )
        .await
}

async fn account_of(world: &World, outcome: SignInOutcome) -> (Uuid, String) {
    let SignInOutcome::Session { tokens, return_to } = outcome else {
        panic!("expected a session outcome");
    };
    let principal = authenticator(&world.harness)
        .authenticate(Credential::Session(tokens.access))
        .await
        .expect("the issued access token authenticates");
    (principal.account_id, return_to)
}

async fn signs_in_known_identity_and_keeps_the_account_when_the_email_changes(
    provider: &str,
    subject: &str,
) {
    // Arrange
    let mut world = world().await;
    let account = world.known_identity(provider, subject).await;
    world.idp.behave(|behaviour| {
        behaviour.github_emails = vec![("first@x.test".to_owned(), true, true)];
    });

    // Act
    let first = sign_in(&world, provider, subject, "first@x.test")
        .await
        .expect("first sign-in");
    world.idp.behave(|behaviour| {
        behaviour.github_emails = vec![("second@x.test".to_owned(), true, true)];
    });
    let second = sign_in(&world, provider, subject, "second@x.test")
        .await
        .expect("sign-in with a changed email");

    // Assert
    assert_eq!(
        account_of(&world, first).await,
        (account, "/files".to_owned())
    );
    assert_eq!(account_of(&world, second).await.0, account);
    let counts = world.counts().await;
    assert_eq!(
        (counts.accounts, counts.identities, counts.sessions),
        (1, 1, 2)
    );
    let last_login_set: bool =
        sqlx::query_scalar("SELECT last_login_at IS NOT NULL FROM identities")
            .fetch_one(&mut world.conn)
            .await
            .expect("identity row");
    assert!(last_login_set);
    world.finish().await;
}

#[tokio::test]
async fn oidc_sign_in_gives_the_identitys_account_a_session_and_a_changed_email_adds_no_account() {
    signs_in_known_identity_and_keeps_the_account_when_the_email_changes(OIDC, "sub-1").await;
}

#[tokio::test]
async fn github_sign_in_gives_the_identitys_account_a_session_and_a_changed_email_adds_no_account()
{
    signs_in_known_identity_and_keeps_the_account_when_the_email_changes(GITHUB, "4242").await;
}

#[tokio::test]
async fn sign_in_of_an_unknown_identity_is_invitation_required_and_creates_nothing() {
    // Arrange
    let mut world = world().await;
    let before = world.counts().await;

    // Act
    let result = sign_in(&world, OIDC, "stranger", "stranger@x.test").await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::InvitationRequired)),
        "{result:?}"
    );
    let mut after = world.counts().await;
    assert_eq!(after.oauth_states, 0, "the state was consumed");
    after.oauth_states = before.oauth_states;
    assert_eq!(after, before);
    world.finish().await;
}

#[tokio::test]
async fn sign_in_never_finds_an_account_by_email() {
    // Arrange
    let mut world = world().await;
    world.known_identity(OIDC, "owner-of-the-email").await;

    // Act
    let result = sign_in(&world, OIDC, "other-subject", "owner@x.test").await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::InvitationRequired)),
        "{result:?}"
    );
    assert_eq!(world.counts().await.sessions, 0);
    world.finish().await;
}

#[tokio::test]
async fn sign_in_of_a_disabled_account_is_refused_without_a_session() {
    // Arrange
    let mut world = world().await;
    let account = world.known_identity(OIDC, "sub-disabled").await;
    support::fixtures::set_account_state(&mut world.conn, account, "disabled").await;

    // Act
    let result = sign_in(&world, OIDC, "sub-disabled", "d@x.test").await;

    // Assert
    assert!(
        matches!(
            result,
            Err(SignInError::TrustedFailure {
                code: ErrorCode::AccountNotActive
            })
        ),
        "{result:?}"
    );
    assert_eq!(world.counts().await.sessions, 0);
    world.finish().await;
}

#[tokio::test]
async fn a_completed_sign_in_leaves_no_plaintext_token_state_or_code_in_the_database() {
    // Arrange
    let mut world = world().await;
    world.known_identity(OIDC, "sub-secrets").await;
    let (url, binding) = world.begin(OIDC).await;
    let state_dump: String = sqlx::query_scalar("SELECT row_to_json(o)::text FROM oauth_states o")
        .fetch_one(&mut world.conn)
        .await
        .expect("state row");
    let callback = world.idp.authorize(&url, "sub-secrets");
    let code = callback[0].1.clone();
    let state = callback[1].1.clone();

    // Act
    let outcome = world
        .service
        .complete(
            OIDC,
            params(callback),
            &binding,
            support::sign_in::HOST,
            support::sign_in::CLIENT_IP,
        )
        .await
        .expect("sign-in");

    // Assert
    let SignInOutcome::Session { tokens, .. } = outcome else {
        panic!("expected a session outcome");
    };
    let (access, refresh) = (tokens.access.expose(), tokens.refresh.expose());
    let (dump, access_hashed, refresh_hashed): (String, bool, bool) = sqlx::query_as(
        "SELECT row_to_json(s)::text, token_hash = sha256(convert_to($1, 'UTF8')), \
         refresh_hash = sha256(convert_to($2, 'UTF8')) FROM sessions s",
    )
    .bind(access)
    .bind(refresh)
    .fetch_one(&mut world.conn)
    .await
    .expect("session row");
    assert!(
        access_hashed && refresh_hashed,
        "only SHA-256 of the tokens is stored"
    );
    for (name, secret) in [("access", access), ("refresh", refresh), ("code", &code)] {
        assert!(!dump.contains(secret), "{name} in sessions");
        assert!(!state_dump.contains(secret), "{name} in oauth_states");
    }
    for (name, secret) in [("state", state.as_str()), ("binding", binding.0.as_str())] {
        assert!(!state_dump.contains(secret), "{name} in oauth_states");
        assert!(!dump.contains(secret), "{name} in sessions");
    }
    assert_eq!(world.counts().await.oauth_states, 0);
    world.finish().await;
}
