//! Member and operator admin invites (AZ-3688): AC-3 (AC-23, RESTRICT-18, FT-P-65, FT-N-21: plaintext once, list
//! without a token, single use, revoked refused), the revoke outcome of an unknown, revoked or redeemed invite
//! (owner decision 2026-11-13) and AC-4 (operator admin invite, no creator), with the real hooks of 02, 05 and
//! 07a on PostgreSQL 18.

mod support;

use std::time::Duration;

use contracts::{InviteError, InviteService, OperatorIdentityOps, SignInError, SignInOutcome};
use support::START;
use support::invites::{invites, sha256_of, token_rows};
use support::registration::{INVITE, Rig, sign_in};
use support::sign_in::OIDC;
use uuid::Uuid;

const POOL: u32 = 5;

/// An admin account created through an operator admin invite; returns its id.
async fn admin(rig: &mut Rig) -> Uuid {
    let token = invites(&rig.world.harness)
        .create_admin_invite()
        .await
        .expect("operator admin invite");
    sign_in(&rig.world, OIDC, "admin-1", Some(token.0.expose()), |_| {})
        .await
        .expect("the admin signs in");
    rig.account_of(OIDC, "admin-1")
        .await
        .expect("the admin account")
        .0
}

#[tokio::test]
async fn a_member_invite_is_returned_once_listed_without_its_token_and_redeems_once() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let admin = admin(&mut rig).await;
    let service = invites(&rig.world.harness);
    let token = service
        .create_invite(admin, "for Bob")
        .await
        .expect("create");

    // Act
    let listed = service.list(admin).await.expect("list");
    let first = sign_in(&rig.world, OIDC, "bob", Some(token.0.expose()), |_| {}).await;
    let reuse = sign_in(&rig.world, OIDC, "eve", Some(token.0.expose()), |_| {}).await;

    // Assert
    let hash = sha256_of(&mut rig.world.conn, token.0.expose()).await;
    let row = token_rows(&mut rig.world.conn)
        .await
        .into_iter()
        .find(|row| row.token_hash == hash)
        .expect("the stored row has the token's hash");
    assert_eq!(
        (row.kind.as_str(), row.grants_role.as_str(), row.created_by),
        ("invite", "user", Some(admin))
    );
    assert_eq!(row.note.as_deref(), Some("for Bob"));
    let mine = listed
        .iter()
        .find(|info| info.id == row.id)
        .expect("listed");
    assert_eq!(mine.note, "for Bob");
    assert_eq!(mine.created_at, START);
    assert_eq!(mine.redeemed_at, None);
    assert!(!format!("{listed:?}").contains(token.0.expose()));
    assert!(
        matches!(first, Ok(SignInOutcome::Session { .. })),
        "{first:?}"
    );
    let (bob, role, _) = rig.account_of(OIDC, "bob").await.expect("Bob's account");
    assert_eq!(role, "user");
    assert_eq!(rig.redemption_of(row.id).await, (Some(bob), true));
    assert!(
        matches!(reuse, Err(SignInError::InvitationRequired)),
        "{reuse:?}"
    );
    assert!(rig.account_of(OIDC, "eve").await.is_none());
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn the_list_shows_redemption_and_expiry_and_never_the_setup_token_or_a_rebind_invite() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let admin = admin(&mut rig).await;
    let service = invites(&rig.world.harness);
    rig.world.harness.clock.tick(Duration::from_secs(1));
    service.create_invite(admin, "later").await.expect("create");
    service.reset_admin(admin).await.expect("reset");

    // Act
    let listed = service.list(admin).await.expect("list");

    // Assert
    let notes: Vec<(&str, bool)> = listed
        .iter()
        .map(|info| (info.note.as_str(), info.redeemed_at.is_some()))
        .collect();
    assert_eq!(notes, vec![("later", false), ("", true)]);
    let ttl = rig.world.harness.config.invite_ttl;
    assert_eq!(
        Some(listed[0].expires_at),
        listed[0].created_at.checked_add(ttl)
    );
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn a_revoked_invite_gives_invitation_required() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let admin = admin(&mut rig).await;
    let service = invites(&rig.world.harness);
    let token = service.create_invite(admin, "").await.expect("create");
    let hash = sha256_of(&mut rig.world.conn, token.0.expose()).await;
    let id = token_rows(&mut rig.world.conn)
        .await
        .into_iter()
        .find(|row| row.token_hash == hash)
        .expect("stored")
        .id;
    service.revoke(admin, id).await.expect("revoke");

    // Act
    let outcome = sign_in(&rig.world, OIDC, "carol", Some(token.0.expose()), |_| {}).await;

    // Assert
    assert!(
        matches!(outcome, Err(SignInError::InvitationRequired)),
        "{outcome:?}"
    );
    assert!(rig.account_of(OIDC, "carol").await.is_none());
    assert!(
        service
            .list(admin)
            .await
            .expect("list")
            .iter()
            .all(|info| info.id != id)
    );
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn revoke_answers_not_found_alike_for_an_unknown_a_revoked_and_a_redeemed_invite() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let admin = admin(&mut rig).await;
    let service = invites(&rig.world.harness);
    let revoked = rig.member_invite("to-revoke").await;
    service.revoke(admin, revoked).await.expect("first revoke");
    let redeemed = rig.member_invite(INVITE).await;
    sign_in(&rig.world, OIDC, "dave", Some(INVITE), |_| {})
        .await
        .expect("redeemed");
    let before = token_rows(&mut rig.world.conn).await;

    // Act
    let outcomes = [
        service.revoke(admin, Uuid::from_u128(42)).await,
        service.revoke(admin, revoked).await,
        service.revoke(admin, redeemed).await,
    ];

    // Assert
    for outcome in &outcomes {
        assert!(matches!(outcome, Err(InviteError::NotFound)), "{outcome:?}");
    }
    assert_eq!(token_rows(&mut rig.world.conn).await, before);
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn an_operator_admin_invite_creates_a_second_admin_and_has_no_creator() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    admin(&mut rig).await;
    let operator = invites(&rig.world.harness);
    let token = operator.create_admin_invite().await.expect("admin invite");

    // Act
    let outcome = sign_in(&rig.world, OIDC, "admin-2", Some(token.0.expose()), |_| {}).await;

    // Assert
    assert!(
        matches!(outcome, Ok(SignInOutcome::Session { .. })),
        "{outcome:?}"
    );
    let (second, role, _) = rig.account_of(OIDC, "admin-2").await.expect("account");
    assert_eq!(role, "admin");
    let admins: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE role = 'admin'")
        .fetch_one(&mut rig.world.conn)
        .await
        .expect("count admins");
    assert_eq!(admins, 2);
    let hash = sha256_of(&mut rig.world.conn, token.0.expose()).await;
    let row = token_rows(&mut rig.world.conn)
        .await
        .into_iter()
        .find(|row| row.token_hash == hash)
        .expect("stored");
    assert_eq!(
        (row.grants_role.as_str(), row.created_by, row.redeemed_by),
        ("admin", None, Some(second))
    );
    drop(operator);
    rig.finish().await;
}
