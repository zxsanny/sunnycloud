//! Invite redemption and account creation (AZ-3685): AC-1 (FT-P-65), AC-2 (FT-N-20, FT-N-21), AC-4 (IT-01-17)
//! and the hook-list cases of the `account_creation_hooks` contract (unset list, second registration), with the
//! real hooks of 02, 05 and 07a on PostgreSQL 18, the manual clock and the in-process provider.

mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{AccountLifecycleHook, Authenticator, Credential, SignInError, SignInOutcome};
use support::START;
use support::auth::authenticator;
use support::registration::{FaultyHook, INVITE, Rig, Rows, sign_in};
use support::sign_in::{GITHUB, OIDC};

const POOL: u32 = 5;
const NOTHING: Rows = Rows {
    accounts: 0,
    identities: 0,
    root_nodes: 0,
    feeds: 0,
    quotas: 0,
    redeemed_invites: 0,
};
const ONE_ACCOUNT: Rows = Rows {
    accounts: 1,
    identities: 1,
    root_nodes: 1,
    feeds: 1,
    quotas: 1,
    redeemed_invites: 1,
};

#[tokio::test]
async fn an_invite_creates_one_account_with_its_identity_root_feed_and_quota_and_a_session() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let invite = rig
        .invite(INVITE, "admin", START.as_unix_micros() + 60_000_000)
        .await;

    // Act
    let outcome = sign_in(&rig.world, OIDC, "new-user", Some(INVITE), |grant| {
        grant.name = Some("Ada Lovelace".to_owned());
    })
    .await;

    // Assert
    let Ok(SignInOutcome::Session { tokens, return_to }) = outcome else {
        panic!("expected a session, got {outcome:?}");
    };
    assert_eq!(return_to, "/files");
    assert_eq!(rig.rows().await, ONE_ACCOUNT);
    let (account, role, display_name) = rig
        .account_of(OIDC, "new-user")
        .await
        .expect("the identity has an account");
    assert_eq!(
        (role.as_str(), display_name.as_str()),
        ("admin", "Ada Lovelace")
    );
    assert_eq!(rig.hook_rows_of(account).await, (1, 1, 1));
    assert_eq!(rig.redemption_of(invite).await, (Some(account), true));
    let principal = authenticator(&rig.world.harness)
        .authenticate(Credential::Session(tokens.access))
        .await
        .expect("the issued access token authenticates");
    assert_eq!(principal.account_id, account);
    rig.finish().await;
}

#[tokio::test]
async fn a_member_invite_gives_the_role_user_and_a_second_sign_in_reaches_the_same_account() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    rig.member_invite(INVITE).await;
    sign_in(&rig.world, GITHUB, "7001", Some(INVITE), |_| {})
        .await
        .expect("redemption");

    // Act
    let again = sign_in(&rig.world, GITHUB, "7001", None, |_| {}).await;

    // Assert
    assert!(
        matches!(again, Ok(SignInOutcome::Session { .. })),
        "{again:?}"
    );
    let (_, role, display_name) = rig.account_of(GITHUB, "7001").await.expect("account");
    assert_eq!((role.as_str(), display_name.as_str()), ("user", "Octo Cat"));
    assert_eq!(rig.rows().await, ONE_ACCOUNT);
    rig.finish().await;
}

#[tokio::test]
async fn without_an_invite_with_an_unknown_one_or_an_expired_one_the_result_is_invitation_required_and_nothing_is_written()
 {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let now = START.as_unix_micros();
    rig.invite("expired-before", "user", now - 1).await;
    rig.invite("expires-now", "user", now).await;
    rig.invite("expires-during", "user", now + 100_000).await;
    let (callback, binding) =
        support::registration::authorized(&rig.world, OIDC, "late", Some("expires-during"), |_| {})
            .await;

    // Act
    let mut results = Vec::new();
    for invite in [
        None,
        Some("unknown-token"),
        Some("expired-before"),
        Some("expires-now"),
    ] {
        results.push(sign_in(&rig.world, OIDC, "stranger", invite, |_| {}).await);
    }
    rig.world.harness.clock.tick(Duration::from_millis(500));
    results.push(
        rig.world
            .service
            .complete(OIDC, callback, &binding, support::sign_in::HOST)
            .await,
    );

    // Assert
    for result in &results {
        assert!(
            matches!(result, Err(SignInError::InvitationRequired)),
            "{result:?}"
        );
    }
    assert_eq!(rig.rows().await, NOTHING);
    rig.finish().await;
}

#[tokio::test]
async fn a_failing_hook_rolls_every_row_back_and_the_invite_stays_redeemable_for_the_retry() {
    // Arrange
    let faulty = FaultyHook::failing_once();
    let extra: Arc<dyn AccountLifecycleHook> = faulty.clone();
    let mut rig = Rig::new(POOL, |_| {}, vec![extra]).await;
    let invite = rig.member_invite(INVITE).await;

    // Act
    let first = sign_in(&rig.world, OIDC, "unlucky", Some(INVITE), |_| {}).await;
    let after_failure = rig.rows().await;
    let invite_after_failure = rig.redemption_of(invite).await;
    let retry = sign_in(&rig.world, OIDC, "unlucky", Some(INVITE), |_| {}).await;

    // Assert
    assert!(
        matches!(first, Err(SignInError::Unexpected(_))),
        "{first:?}"
    );
    assert_eq!(after_failure, NOTHING);
    assert_eq!(invite_after_failure, (None, false));
    assert!(
        matches!(retry, Ok(SignInOutcome::Session { .. })),
        "{retry:?}"
    );
    assert_eq!(rig.rows().await, ONE_ACCOUNT);
    assert_eq!(faulty.calls(), 2);
    rig.finish().await;
}

#[tokio::test]
async fn account_creation_fails_closed_while_no_hook_list_is_registered() {
    // Arrange
    let mut rig = Rig::unregistered(POOL, |_| {}).await;
    let invite = rig.member_invite(INVITE).await;

    // Act
    let result = sign_in(&rig.world, OIDC, "early", Some(INVITE), |_| {}).await;

    // Assert
    assert!(
        matches!(result, Err(SignInError::Unexpected(_))),
        "{result:?}"
    );
    assert_eq!(rig.rows().await, NOTHING);
    assert_eq!(rig.redemption_of(invite).await, (None, false));
    rig.finish().await;
}

#[tokio::test]
async fn a_second_hook_registration_is_refused_and_the_first_list_stays() {
    // Arrange
    let faulty = FaultyHook::failing_once();
    let first: Arc<dyn AccountLifecycleHook> = faulty.clone();
    let mut rig = Rig::new(POOL, |_| {}, vec![first]).await;
    rig.member_invite(INVITE).await;

    // Act
    let second = rig.world.service.register_hooks(rig.real_hooks());
    let result = sign_in(&rig.world, OIDC, "someone", Some(INVITE), |_| {}).await;

    // Assert
    assert!(second.is_err());
    assert!(
        matches!(result, Err(SignInError::Unexpected(_))),
        "{result:?}"
    );
    assert_eq!(faulty.calls(), 1, "the first list ran");
    rig.finish().await;
}

#[tokio::test]
async fn the_display_name_is_the_name_else_the_username_else_user() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    for token in ["i-1", "i-2", "i-3", "i-4"] {
        rig.member_invite(token).await;
    }

    // Act
    sign_in(&rig.world, OIDC, "named", Some("i-1"), |grant| {
        grant.name = Some("Grace".to_owned());
        grant.preferred_username = Some("grace-h".to_owned());
    })
    .await
    .expect("named");
    sign_in(&rig.world, OIDC, "username-only", Some("i-2"), |grant| {
        grant.preferred_username = Some("grace-h".to_owned());
    })
    .await
    .expect("username only");
    sign_in(&rig.world, OIDC, "anonymous", Some("i-3"), |_| {})
        .await
        .expect("no name");
    rig.world
        .idp
        .behave(|behaviour| behaviour.github_no_name = true);
    sign_in(&rig.world, GITHUB, "7002", Some("i-4"), |_| {})
        .await
        .expect("github login only");

    // Assert
    let mut names = Vec::new();
    for (provider, subject) in [
        (OIDC, "named"),
        (OIDC, "username-only"),
        (OIDC, "anonymous"),
        (GITHUB, "7002"),
    ] {
        names.push(rig.account_of(provider, subject).await.expect("account").2);
    }
    assert_eq!(names, ["Grace", "grace-h", "User", "octo"]);
    rig.finish().await;
}
