//! The setup token (AZ-3688): AC-1 (IT-01-7 main path, AC-23, FT-P-64, FT-N-20, NFT-SEC-01), AC-2 (IT-01-7
//! replacement, expiry and the account-present refusal, FT-P-78) and concurrent setup redemptions (FT-N-21), with
//! the real hooks of 02, 05 and 07a on PostgreSQL 18, the manual clock and the in-process provider.

mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{OperatorIdentityOps, SignInError, SignInOutcome};
use support::invites::{invites, sha256_of, sign_in_with_setup, token_rows};
use support::registration::{INVITE, Rig, sign_in};
use support::sign_in::OIDC;
use tokio::sync::Barrier;
use tokio::task::JoinSet;

const POOL: u32 = 5;
const SHORT_TTL: Duration = Duration::from_secs(10);

async fn account_count(rig: &mut Rig) -> i64 {
    rig.rows().await.accounts
}

#[tokio::test]
async fn the_setup_token_creates_one_admin_once_and_nothing_without_or_with_a_wrong_token() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let token = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a token while no account exists");

    // Act
    let without = sign_in(&rig.world, OIDC, "first", None, |_| {}).await;
    let without_accounts = account_count(&mut rig).await;
    let wrong = sign_in_with_setup(&rig.world, OIDC, "first", "sc_set_not-the-token").await;
    let wrong_accounts = account_count(&mut rig).await;
    let right = sign_in_with_setup(&rig.world, OIDC, "first", token.expose()).await;
    let again = sign_in_with_setup(&rig.world, OIDC, "second", token.expose()).await;

    // Assert
    assert!(
        matches!(without, Err(SignInError::InvitationRequired)),
        "{without:?}"
    );
    assert!(
        matches!(wrong, Err(SignInError::InvitationRequired)),
        "{wrong:?}"
    );
    assert_eq!((without_accounts, wrong_accounts), (0, 0));
    assert!(
        matches!(right, Ok(SignInOutcome::Session { .. })),
        "{right:?}"
    );
    assert!(
        matches!(again, Err(SignInError::InvitationRequired)),
        "{again:?}"
    );
    let (account, role, _) = rig.account_of(OIDC, "first").await.expect("the admin");
    assert_eq!(role, "admin");
    assert_eq!(account_count(&mut rig).await, 1);
    let rows = token_rows(&mut rig.world.conn).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "setup");
    assert_eq!(rows[0].redeemed_by, Some(account));
    assert_eq!(rows[0].created_by, None);
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn only_the_hash_of_the_setup_token_is_stored_and_the_token_debug_is_redacted() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);

    // Act
    let token = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a token");

    // Assert
    let expected = sha256_of(&mut rig.world.conn, token.expose()).await;
    let rows = token_rows(&mut rig.world.conn).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].token_hash, expected);
    assert!(!format!("{token:?}").contains(token.expose()));
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn a_second_ensure_replaces_the_first_token_and_only_the_newest_redeems() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let first = operator.ensure_setup_token().await.expect("ensure");
    let second = operator.ensure_setup_token().await.expect("ensure again");
    let (first, second) = (first.expect("first"), second.expect("second"));

    // Act
    let with_first = sign_in_with_setup(&rig.world, OIDC, "first", first.expose()).await;
    let with_second = sign_in_with_setup(&rig.world, OIDC, "first", second.expose()).await;

    // Assert
    assert!(
        matches!(with_first, Err(SignInError::InvitationRequired)),
        "{with_first:?}"
    );
    assert!(
        matches!(with_second, Ok(SignInOutcome::Session { .. })),
        "{with_second:?}"
    );
    let setup_rows = token_rows(&mut rig.world.conn).await;
    assert_eq!(setup_rows.len(), 1, "the replaced token row is gone");
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn an_expired_setup_token_gives_invitation_required_and_creates_nothing() {
    // Arrange
    let mut rig = Rig::new(
        POOL,
        |config| config.setup_token_ttl = SHORT_TTL,
        Vec::new(),
    )
    .await;
    let operator = invites(&rig.world.harness);
    let token = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a token");
    rig.world.harness.clock.tick(SHORT_TTL);

    // Act
    let outcome = sign_in_with_setup(&rig.world, OIDC, "late", token.expose()).await;

    // Assert
    assert!(
        matches!(outcome, Err(SignInError::InvitationRequired)),
        "{outcome:?}"
    );
    assert_eq!(account_count(&mut rig).await, 0);
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn ensure_returns_nothing_and_stores_nothing_once_an_account_exists() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    rig.member_invite(INVITE).await;
    sign_in(&rig.world, OIDC, "member", Some(INVITE), |_| {})
        .await
        .expect("an account exists");
    let before = token_rows(&mut rig.world.conn).await;
    let operator = invites(&rig.world.harness);

    // Act
    let token = operator.ensure_setup_token().await.expect("ensure");

    // Assert
    assert!(token.is_none());
    assert_eq!(token_rows(&mut rig.world.conn).await, before);
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn a_setup_token_issued_before_the_first_account_no_longer_redeems_once_an_account_exists() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let token = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a token");
    rig.member_invite(INVITE).await;
    sign_in(&rig.world, OIDC, "member", Some(INVITE), |_| {})
        .await
        .expect("an account exists");

    // Act
    let outcome = sign_in_with_setup(&rig.world, OIDC, "late", token.expose()).await;

    // Assert
    assert!(
        matches!(outcome, Err(SignInError::InvitationRequired)),
        "{outcome:?}"
    );
    assert_eq!(account_count(&mut rig).await, 1);
    drop(operator);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_setup_redemptions_leave_exactly_one_admin() {
    // Arrange
    const RACERS: usize = 8;
    let mut rig = Rig::new(RACERS as u32 + 4, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let token = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a token");
    let mut callbacks = Vec::new();
    for racer in 0..RACERS {
        let mut request = support::sign_in::begin_request(OIDC, "/files");
        request.setup_token = Some(sc_ids::RawToken::from_presented(
            sc_ids::TokenKind::Setup,
            token.expose().to_owned(),
        ));
        let redirect = rig.world.service.begin(request).await.expect("begin");
        let callback =
            rig.world
                .idp
                .authorize_with(&redirect.url, &format!("{}", 9000 + racer), |_| {});
        callbacks.push((support::sign_in::params(callback), redirect.browser_binding));
    }
    let service = Arc::new(rig.new_service());
    service
        .register_hooks(rig.real_hooks())
        .expect("register the hooks");
    let barrier = Arc::new(Barrier::new(RACERS));

    // Act
    let mut calls = JoinSet::new();
    for (callback, binding) in callbacks {
        let (service, barrier) = (Arc::clone(&service), Arc::clone(&barrier));
        calls.spawn(async move {
            barrier.wait().await;
            service
                .complete(
                    OIDC,
                    callback,
                    &binding,
                    support::sign_in::HOST,
                    support::sign_in::CLIENT_IP,
                )
                .await
        });
    }
    let results = calls.join_all().await;

    // Assert
    let sessions = results
        .iter()
        .filter(|result| matches!(result, Ok(SignInOutcome::Session { .. })))
        .count();
    let refused = results
        .iter()
        .filter(|result| matches!(result, Err(SignInError::InvitationRequired)))
        .count();
    assert_eq!((sessions, refused), (1, RACERS - 1), "{results:?}");
    let rows = rig.rows().await;
    assert_eq!((rows.accounts, rows.root_nodes, rows.quotas), (1, 1, 1));
    drop(service);
    drop(operator);
    rig.finish().await;
}
