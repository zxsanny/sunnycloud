//! AZ-4024 AC-3, AC-4 and AC-7 (AC-56, AC-55, FT-N-23, NFT-SEC-02, NFT-RES-24, NFT-SEC-30) with Sign in with
//! Apple: every negative callback ends in its documented error and creates no session, account, identity or
//! login code; a relay or unverified address is never a key; a stalling Apple and a key-set URL that redirects
//! leave nothing behind. A known identity exists in every test, so a failure is caused by the mutation.

mod support;

use std::time::{Duration, Instant};

use contracts::{BrowserBinding, SignInError, SignInOutcome};
use sc_error::ErrorCode;
use support::apple::{APPLE, TeamKey, apple_world};
use support::provider::{Behaviour, FakeProvider, Grant};
use support::registration::{INVITE, Rig, sign_in};
use support::sign_in::{CLIENT_IP, Counts, HOST, World, params};
use uuid::Uuid;

const SUBJECT: &str = "apple-sub-1";
const VICTIM: &str = "victim@example.test";
const RELAY: &str = "x1@privaterelay.appleid.com";

/// Rows a failed callback must not create.
fn created(counts: &Counts) -> [i64; 4] {
    [
        counts.sessions,
        counts.accounts,
        counts.identities,
        counts.login_codes,
    ]
}

async fn known_world() -> World {
    let mut world = apple_world(&TeamKey::generate(), |_| {}).await;
    world.known_identity(APPLE, SUBJECT).await;
    world
}

async fn authorized(world: &World) -> (Vec<(String, String)>, BrowserBinding) {
    let (url, binding) = world.begin(APPLE).await;
    (world.idp.authorize(&url, SUBJECT), binding)
}

async fn complete(
    world: &World,
    callback: Vec<(String, String)>,
    binding: &BrowserBinding,
) -> Result<SignInOutcome, SignInError> {
    world
        .service
        .complete(APPLE, params(callback), binding, HOST, CLIENT_IP)
        .await
}

fn is_trusted_failure(result: &Result<SignInOutcome, SignInError>) -> bool {
    matches!(
        result,
        Err(SignInError::TrustedFailure {
            code: ErrorCode::Unauthenticated
        })
    )
}

async fn trusted_failure_for(change: impl FnOnce(&mut Grant)) {
    // Arrange
    let mut world = known_world().await;
    let (url, binding) = world.begin(APPLE).await;
    let callback = world.idp.authorize_with(&url, SUBJECT, change);
    let before = world.counts().await;

    // Act
    let result = complete(&world, callback, &binding).await;

    // Assert
    assert!(is_trusted_failure(&result), "{result:?}");
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}

#[tokio::test]
async fn a_missing_replayed_foreign_or_expired_state_is_untrusted_state_and_creates_nothing() {
    // Arrange
    let mut world = known_world().await;
    let (callback, binding) = authorized(&world).await;
    let without_state = vec![callback[0].clone()];
    let (other_callback, other_binding) = authorized(&world).await;
    let (expired_callback, expired_binding) = authorized(&world).await;
    let before = world.counts().await;

    // Act
    let missing = complete(&world, without_state, &binding).await;
    let foreign = complete(
        &world,
        other_callback.clone(),
        &BrowserBinding("another-browser".to_owned()),
    )
    .await;
    let genuine = complete(&world, other_callback, &other_binding).await;
    let first = complete(&world, callback.clone(), &binding).await;
    let replayed = complete(&world, callback, &binding).await;
    world
        .harness
        .clock
        .tick(world.harness.config.oauth_state_ttl + Duration::from_millis(1));
    let expired = complete(&world, expired_callback, &expired_binding).await;

    // Assert
    for (name, result) in [
        ("missing", &missing),
        ("foreign", &foreign),
        ("replayed", &replayed),
        ("expired", &expired),
    ] {
        assert!(
            matches!(result, Err(SignInError::UntrustedState)),
            "{name}: {result:?}"
        );
    }
    for result in [&genuine, &first] {
        assert!(
            matches!(result, Ok(SignInOutcome::Session { .. })),
            "{result:?}"
        );
    }
    let after = world.counts().await;
    assert_eq!(
        (after.accounts, after.identities, after.login_codes),
        (before.accounts, before.identities, before.login_codes)
    );
    assert_eq!(
        after.sessions,
        before.sessions + 2,
        "only the two genuine sign-ins made a session"
    );
    world.finish().await;
}

#[tokio::test]
async fn a_wrong_nonce_audience_or_issuer_is_a_trusted_failure_and_creates_nothing() {
    trusted_failure_for(|grant| grant.nonce = "forged-nonce".to_owned()).await;
    trusted_failure_for(|grant| grant.audience = "another-services-id".to_owned()).await;
    trusted_failure_for(|grant| grant.issuer = "https://evil.test".to_owned()).await;
}

#[tokio::test]
async fn an_id_token_with_a_bad_signature_is_a_trusted_failure_and_creates_nothing() {
    // Arrange
    let mut world = known_world().await;
    let (callback, binding) = authorized(&world).await;
    world
        .idp
        .behave(|behaviour| behaviour.corrupt_id_token_signature = true);
    let before = world.counts().await;

    // Act
    let result = complete(&world, callback, &binding).await;

    // Assert
    assert!(is_trusted_failure(&result), "{result:?}");
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}

#[tokio::test]
async fn a_reused_apple_code_is_a_trusted_failure_and_creates_nothing() {
    // Arrange
    let mut world = known_world().await;
    let (callback, binding) = authorized(&world).await;
    let first = complete(&world, callback.clone(), &binding).await;
    assert!(
        matches!(first, Ok(SignInOutcome::Session { .. })),
        "{first:?}"
    );
    let (second_url, second_binding) = world.begin(APPLE).await;
    let second_state = world.idp.authorize(&second_url, SUBJECT)[1].clone();
    let reused = vec![callback[0].clone(), second_state];
    let before = world.counts().await;

    // Act
    let result = complete(&world, reused, &second_binding).await;

    // Assert
    assert!(is_trusted_failure(&result), "{result:?}");
    assert_eq!(created(&world.counts().await), created(&before));
    world.finish().await;
}

/// The registration rig with `apple` as the only provider, the given allowlist and a victim account that holds
/// `VICTIM` as its verified email.
async fn apple_rig(allowlist: Vec<String>) -> (Rig, Uuid) {
    let team = TeamKey::generate();
    let mut rig = Rig::new(
        5,
        |config| {
            config.providers = vec![team.provider()];
            config.email_allowlist = allowlist;
        },
        Vec::new(),
    )
    .await;
    rig.world
        .idp
        .check_client_secrets(team.check(rig.world.harness.clock.clone()));
    let victim = rig.world.known_identity(APPLE, "victim-sub").await;
    sqlx::query("UPDATE identities SET email = $1, email_verified = true WHERE account_id = $2")
        .bind(VICTIM)
        .bind(victim)
        .execute(&mut rig.world.conn)
        .await
        .expect("store the victim's email");
    rig.member_invite(INVITE).await;
    (rig, victim)
}

#[tokio::test]
async fn with_an_allowlist_a_relay_or_unverified_apple_address_is_no_email_and_creates_and_links_nothing()
 {
    // Arrange
    let (mut rig, victim) = apple_rig(vec![VICTIM.to_owned(), RELAY.to_owned()]).await;
    let before = rig.world.counts().await;
    let attempts = [
        (None, (RELAY, false)),
        (None, (VICTIM, false)),
        (None, (RELAY, true)),
        (Some(INVITE), (RELAY, false)),
        (Some(INVITE), (VICTIM, false)),
        (Some(INVITE), (RELAY, true)),
    ];

    for (index, (invite, (address, verified))) in attempts.into_iter().enumerate() {
        // Act
        let result = sign_in(
            &rig.world,
            APPLE,
            &format!("attacker-{index}"),
            invite,
            |grant| {
                grant.email = Some((address.to_owned(), verified));
            },
        )
        .await;

        // Assert
        assert!(
            matches!(result, Err(SignInError::InvitationRequired)),
            "{address} verified={verified} invite={invite:?}: {result:?}"
        );
    }
    let after = rig.world.counts().await;
    assert_eq!(
        (after.accounts, after.identities),
        (before.accounts, before.identities)
    );
    let linked: i64 = sqlx::query_scalar("SELECT count(*) FROM identities WHERE account_id = $1")
        .bind(victim)
        .fetch_one(&mut rig.world.conn)
        .await
        .expect("count the victim's identities");
    assert_eq!(linked, 1, "nothing was linked to the victim");
    rig.finish().await;
}

#[tokio::test]
async fn without_an_allowlist_an_apple_invite_creates_a_separate_account_that_holds_no_email_and_links_nothing()
 {
    // Arrange
    let (mut rig, victim) = apple_rig(Vec::new()).await;
    let uninvited = sign_in(&rig.world, APPLE, "attacker-1", None, |grant| {
        grant.email = Some((VICTIM.to_owned(), false));
    })
    .await;
    let before = rig.world.counts().await;

    // Act
    let redeemed = sign_in(&rig.world, APPLE, "newcomer", Some(INVITE), |grant| {
        grant.email = Some((RELAY.to_owned(), false));
    })
    .await;

    // Assert
    assert!(
        matches!(uninvited, Err(SignInError::InvitationRequired)),
        "{uninvited:?}"
    );
    assert!(
        matches!(redeemed, Ok(SignInOutcome::Session { .. })),
        "{redeemed:?}"
    );
    let (account, _, _) = rig
        .account_of(APPLE, "newcomer")
        .await
        .expect("new account");
    assert_ne!(account, victim);
    let after = rig.world.counts().await;
    assert_eq!(
        (after.accounts, after.identities),
        (before.accounts + 1, before.identities + 1)
    );
    let (victims, stored_emails): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE account_id = $1), count(*) FILTER (WHERE account_id = $2 AND email IS NOT NULL) \
         FROM identities",
    )
    .bind(victim)
    .bind(account)
    .fetch_one(&mut rig.world.conn)
    .await
    .expect("count identities");
    assert_eq!(
        (victims, stored_emails),
        (1, 0),
        "nothing linked to the victim, no address stored"
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_stalling_apple_gives_provider_unavailable_within_the_timeout_and_nothing_changes() {
    // Arrange
    const TIMEOUT: Duration = Duration::from_millis(300);
    let mut world = apple_world(&TeamKey::generate(), |config| {
        config.provider_timeout = TIMEOUT
    })
    .await;
    world.known_identity(APPLE, SUBJECT).await;
    let (callback, binding) = authorized(&world).await;
    world
        .idp
        .behave(|behaviour| behaviour.stall_token_endpoint = true);
    let before = world.counts().await;

    // Act
    let started = Instant::now();
    let result = complete(&world, callback, &binding).await;
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
    world.finish().await;
}

#[tokio::test]
async fn a_key_set_url_that_redirects_and_a_token_redirect_are_not_followed_and_the_canary_gets_nothing()
 {
    // Arrange
    let canary = FakeProvider::start(0).await;
    let mut world = known_world().await;
    let behaviours = [
        Behaviour {
            jwks_redirect: Some(format!("{}/canary", canary.base_url())),
            ..Behaviour::default()
        },
        Behaviour {
            token_redirect: Some(format!("{}/canary", canary.base_url())),
            ..Behaviour::default()
        },
    ];

    for behaviour in behaviours {
        world.idp.behave(|current| *current = Behaviour::default());
        let (callback, binding) = authorized(&world).await;
        world.idp.behave(|current| *current = behaviour);

        // Act
        let result = complete(&world, callback, &binding).await;

        // Assert
        assert!(
            matches!(result, Err(SignInError::ProviderUnavailable)),
            "{result:?}"
        );
    }
    assert_eq!(canary.hits(""), 0, "the canary received a request");
    assert_eq!(world.counts().await.sessions, 0);
    world.finish().await;
}
