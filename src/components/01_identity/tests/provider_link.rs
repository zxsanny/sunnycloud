//! AZ-3694 (E-15/c14): linking another provider identity to the signed-in account, never by email (AC-55), with
//! the sign-in state rules (AC-56), and the provider list with the setup state (AC-19). Real PostgreSQL, manual
//! clock, the in-process provider as `github` and `oidc`.

mod support;

use contracts::{
    BrowserBinding, CallbackParams, Linked, Principal, PrincipalKind, ProviderInfo, SignInError,
    SignInOutcome,
};
use sqlx::PgConnection;
use support::audit;
use support::provider::Grant;
use support::registration::sign_in;
use support::sessions::principal;
use support::sign_in::{CLIENT_IP, GITHUB, HOST, OIDC, World, params, world, world_with};
use uuid::Uuid;

const VICTIM: &str = "victim@example.test";
/// GitHub user ids are numbers.
const GITHUB_ID: &str = "1001";

fn session_of(account: Uuid) -> Principal {
    principal(account, Uuid::new_v4(), PrincipalKind::Session)
}

/// `begin_link` for `principal` and the provider login of `subject`: the callback and its binding.
async fn link_callback(
    world: &World,
    principal: &Principal,
    provider: &str,
    subject: &str,
    change: impl FnOnce(&mut Grant),
) -> (CallbackParams, BrowserBinding) {
    let redirect = world
        .service
        .begin_link(principal, provider)
        .await
        .expect("begin_link");
    let callback = world.idp.authorize_with(&redirect.url, subject, change);
    (params(callback), redirect.browser_binding)
}

async fn link(
    world: &World,
    principal: &Principal,
    provider: &str,
    subject: &str,
) -> Result<Linked, SignInError> {
    let (callback, binding) = link_callback(world, principal, provider, subject, |_| {}).await;
    world
        .service
        .complete_link(principal, provider, callback, &binding, HOST, CLIENT_IP)
        .await
}

/// `(provider, provider_user_id, email)` of the account's identities, in that order.
async fn identities(
    conn: &mut PgConnection,
    account: Uuid,
) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT provider, provider_user_id, email FROM identities WHERE account_id = $1 \
         ORDER BY provider, provider_user_id",
    )
    .bind(account)
    .fetch_all(conn)
    .await
    .expect("read identities")
}

/// The account the session of the latest sign-in belongs to.
async fn signed_in_account(world: &mut World, provider: &str, subject: &str) -> Uuid {
    let outcome = sign_in(world, provider, subject, None, |_| {}).await;
    assert!(
        matches!(outcome, Ok(SignInOutcome::Session { .. })),
        "{outcome:?}"
    );
    sqlx::query_scalar("SELECT account_id FROM sessions ORDER BY created_at DESC, id LIMIT 1")
        .fetch_one(&mut world.conn)
        .await
        .expect("read the new session")
}

fn pair(provider: &str, subject: &str) -> (String, String, Option<String>) {
    (provider.to_owned(), subject.to_owned(), None)
}

#[tokio::test]
async fn a_second_provider_is_linked_and_either_identity_signs_in_to_the_same_account() {
    // Arrange
    let mut world = world().await;
    let account = world.known_identity(GITHUB, GITHUB_ID).await;
    let session = session_of(account);

    // Act
    let linked = link(&world, &session, OIDC, "oidc-a").await;
    let again = link(&world, &session, OIDC, "oidc-a").await;
    let via_github = signed_in_account(&mut world, GITHUB, GITHUB_ID).await;
    let via_oidc = signed_in_account(&mut world, OIDC, "oidc-a").await;

    // Assert
    assert!(matches!(linked, Ok(Linked)), "{linked:?}");
    assert!(
        matches!(again, Ok(Linked)),
        "an existing link is an idempotent success: {again:?}"
    );
    assert_eq!(
        identities(&mut world.conn, account).await,
        vec![pair(GITHUB, GITHUB_ID), pair(OIDC, "oidc-a")]
    );
    assert_eq!((via_github, via_oidc), (account, account));
    let events: Vec<_> = audit::of(&mut world.conn, "sign_in")
        .await
        .into_iter()
        .filter(|row| row.detail.as_deref() == Some(r#"{"action": "provider_linked"}"#))
        .collect();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].actor_account_id, Some(account));
    assert_eq!(
        events[0].actor_session_id,
        Some(session.session_or_device_id)
    );
    world.finish().await;
}

#[tokio::test]
async fn an_identity_of_another_account_is_taken_and_neither_account_changes() {
    // Arrange
    let mut world = world().await;
    let a = world.known_identity(GITHUB, "gh-a").await;
    let b = world.known_identity(OIDC, "oidc-y").await;

    // Act
    let result = link(&world, &session_of(a), OIDC, "oidc-y").await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::IdentityTaken)),
        "{result:?}"
    );
    assert_eq!(
        identities(&mut world.conn, a).await,
        vec![pair(GITHUB, "gh-a")]
    );
    assert_eq!(
        identities(&mut world.conn, b).await,
        vec![pair(OIDC, "oidc-y")]
    );
    assert!(audit::rows(&mut world.conn).await.is_empty());
    world.finish().await;
}

#[tokio::test]
async fn a_victims_unverified_email_links_nothing_to_the_victim_and_no_email_is_stored() {
    // Arrange
    let mut world = world_with(|config| config.email_allowlist = vec![VICTIM.to_owned()]).await;
    let victim = world.known_identity(OIDC, "victim-sub").await;
    sqlx::query("UPDATE identities SET email = $1, email_verified = true WHERE account_id = $2")
        .bind(VICTIM)
        .bind(victim)
        .execute(&mut world.conn)
        .await
        .expect("store the victim's email");
    let attacker = world.known_identity(GITHUB, "gh-attacker").await;
    let session = session_of(attacker);
    let (callback, binding) = link_callback(&world, &session, OIDC, "attacker-new", |grant| {
        grant.email = Some((VICTIM.to_owned(), false));
    })
    .await;

    // Act
    let result = world
        .service
        .complete_link(&session, OIDC, callback, &binding, HOST, CLIENT_IP)
        .await;

    // Assert
    assert!(matches!(result, Ok(Linked)), "{result:?}");
    assert_eq!(
        identities(&mut world.conn, attacker).await,
        vec![pair(GITHUB, "gh-attacker"), pair(OIDC, "attacker-new")]
    );
    assert_eq!(
        identities(&mut world.conn, victim).await,
        vec![(
            OIDC.to_owned(),
            "victim-sub".to_owned(),
            Some(VICTIM.to_owned())
        )]
    );
    world.finish().await;
}

#[tokio::test]
async fn replayed_foreign_cross_account_sign_in_and_expired_link_callbacks_are_untrusted_and_link_nothing()
 {
    // Arrange
    let mut world = world().await;
    let a = world.known_identity(GITHUB, "gh-a").await;
    let other = world.known_identity(GITHUB, "gh-other").await;
    let session = session_of(a);
    let (replayed, replay_binding) = link_callback(&world, &session, OIDC, "s1", |_| {}).await;
    let replay_copy = CallbackParams(replayed.0.clone());
    let (foreign, _) = link_callback(&world, &session, OIDC, "s2", |_| {}).await;
    let (cross, cross_binding) = link_callback(&world, &session, OIDC, "s3", |_| {}).await;
    let (as_sign_in, sign_in_binding) = link_callback(&world, &session, OIDC, "s4", |_| {}).await;
    let (expired, expired_binding) = link_callback(&world, &session, OIDC, "s5", |_| {}).await;
    let service = &world.service;

    // Act
    let first = service
        .complete_link(&session, OIDC, replayed, &replay_binding, HOST, CLIENT_IP)
        .await;
    let replay = service
        .complete_link(
            &session,
            OIDC,
            replay_copy,
            &replay_binding,
            HOST,
            CLIENT_IP,
        )
        .await;
    let foreign_binding = BrowserBinding("not-the-cookie".to_owned());
    let foreign = service
        .complete_link(&session, OIDC, foreign, &foreign_binding, HOST, CLIENT_IP)
        .await;
    let cross = service
        .complete_link(
            &session_of(other),
            OIDC,
            cross,
            &cross_binding,
            HOST,
            CLIENT_IP,
        )
        .await;
    let as_sign_in = service
        .complete(OIDC, as_sign_in, &sign_in_binding, HOST, CLIENT_IP)
        .await;
    world
        .harness
        .clock
        .tick(world.harness.config.oauth_state_ttl);
    let expired = world
        .service
        .complete_link(&session, OIDC, expired, &expired_binding, HOST, CLIENT_IP)
        .await;

    // Assert
    assert!(matches!(first, Ok(Linked)), "{first:?}");
    for result in [replay, foreign, cross, expired] {
        assert!(
            matches!(result, Err(SignInError::UntrustedState)),
            "{result:?}"
        );
    }
    assert!(
        matches!(as_sign_in, Err(SignInError::UntrustedState)),
        "{as_sign_in:?}"
    );
    assert_eq!(
        identities(&mut world.conn, a).await,
        vec![pair(GITHUB, "gh-a"), pair(OIDC, "s1")]
    );
    assert_eq!(
        identities(&mut world.conn, other).await,
        vec![pair(GITHUB, "gh-other")]
    );
    world.finish().await;
}

#[tokio::test]
async fn the_provider_list_is_the_configured_providers_and_setup_is_required_until_an_account_exists()
 {
    // Arrange
    let mut world = world().await;
    let expected = vec![
        ProviderInfo {
            key: GITHUB.to_owned(),
            display_name: "GitHub".to_owned(),
        },
        ProviderInfo {
            key: OIDC.to_owned(),
            display_name: "OpenID Connect".to_owned(),
        },
    ];

    // Act
    let empty = world.service.list_providers().await.expect("list");
    world.known_identity(GITHUB, "first").await;
    let after = world.service.list_providers().await.expect("list");

    // Assert
    assert_eq!(
        (empty.providers.clone(), empty.setup_required),
        (expected.clone(), true)
    );
    assert_eq!((after.providers, after.setup_required), (expected, false));
    world.finish().await;
}
