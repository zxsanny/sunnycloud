//! The operator's admin reset and the re-bind redemption (AZ-3688): AC-5 (IT-01-9: sessions and device tokens
//! revoked at once, the invite bound to the account, the account kept with a new identity), AC-6 (IT-12-2: the
//! only active admin may be reset, G-12-3), the refusals for an unknown or non-admin account, and the mandatory
//! `admin reset` event, on PostgreSQL 18 with the real hooks of 02, 05 and 07a.

mod support;

use contracts::{
    Authenticator, AuthenticatorError, Credential, OperatorIdentityError, OperatorIdentityOps,
    SignInError, SignInOutcome,
};
use sc_ids::{RawToken, TokenKind};
use support::auth::authenticator;
use support::invites::{
    file_node, identities_of, invites, live_credentials, nodes_of, role_and_state, sha256_of,
    token_rows,
};
use support::registration::{INVITE, Rig, sign_in};
use support::sign_in::OIDC;
use support::{START, audit, fixtures};
use uuid::Uuid;

const POOL: u32 = 5;

/// Admin X signed in through an operator admin invite as `oidc/x-old`, with two more sessions, a device token
/// and a file under its root.
struct AdminX {
    id: Uuid,
    session_token: &'static str,
}

async fn admin_x(rig: &mut Rig) -> AdminX {
    let token = invites(&rig.world.harness)
        .create_admin_invite()
        .await
        .expect("admin invite");
    sign_in(&rig.world, OIDC, "x-old", Some(token.0.expose()), |_| {})
        .await
        .expect("X signs in");
    let id = rig.account_of(OIDC, "x-old").await.expect("X").0;
    let (at, expires) = (START.as_unix_micros(), START.as_unix_micros() + 60_000_000);
    let conn = &mut rig.world.conn;
    fixtures::live_session(conn, id, "x-session-2", "phone", expires, at).await;
    fixtures::named_device(conn, id, "x-device", "laptop", at).await;
    file_node(conn, id, "report.pdf").await;
    AdminX {
        id,
        session_token: "x-session-2",
    }
}

#[tokio::test]
async fn a_reset_revokes_at_once_and_its_invite_rebinds_the_account_to_a_new_identity_keeping_role_and_data()
 {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let x = admin_x(&mut rig).await;
    let nodes_before = nodes_of(&mut rig.world.conn, x.id).await;
    let accounts_before = rig.rows().await.accounts;
    let operator = invites(&rig.world.harness);

    // Act
    let token = operator.reset_admin(x.id).await.expect("reset");
    let after_reset = live_credentials(&mut rig.world.conn, x.id).await;
    let held = authenticator(&rig.world.harness)
        .authenticate(Credential::Session(RawToken::from_presented(
            TokenKind::Session,
            x.session_token.to_owned(),
        )))
        .await;
    let redeemed = sign_in(&rig.world, OIDC, "y-new", Some(token.0.expose()), |_| {}).await;
    let again = sign_in(&rig.world, OIDC, "z-other", Some(token.0.expose()), |_| {}).await;

    // Assert
    assert_eq!(after_reset, (0, 0));
    assert!(
        matches!(held, Err(AuthenticatorError::Unauthenticated)),
        "{held:?}"
    );
    let hash = sha256_of(&mut rig.world.conn, token.0.expose()).await;
    let row = token_rows(&mut rig.world.conn)
        .await
        .into_iter()
        .find(|row| row.token_hash == hash)
        .expect("stored");
    assert_eq!(
        (row.target_account_id, row.created_by, row.redeemed_by),
        (Some(x.id), None, Some(x.id)),
        "{redeemed:?}"
    );
    let Ok(SignInOutcome::Session { tokens, .. }) = redeemed else {
        panic!("expected a session, got {redeemed:?}");
    };
    let principal = authenticator(&rig.world.harness)
        .authenticate(Credential::Session(tokens.access))
        .await
        .expect("Y's session authenticates");
    assert_eq!(principal.account_id, x.id);
    assert_eq!(rig.rows().await.accounts, accounts_before);
    assert_eq!(
        identities_of(&mut rig.world.conn, x.id).await,
        vec![(OIDC.to_owned(), "y-new".to_owned())]
    );
    assert_eq!(
        role_and_state(&mut rig.world.conn, x.id).await,
        ("admin".to_owned(), "active".to_owned())
    );
    assert_eq!(nodes_of(&mut rig.world.conn, x.id).await, nodes_before);
    assert_eq!(rig.hook_rows_of(x.id).await, (1, 1, 1));
    assert!(
        matches!(again, Err(SignInError::InvitationRequired)),
        "{again:?}"
    );
    assert!(rig.account_of(OIDC, "z-other").await.is_none());
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn after_a_reset_the_old_identity_no_longer_signs_in_and_an_earlier_reset_invite_stops_working()
 {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let x = admin_x(&mut rig).await;
    let operator = invites(&rig.world.harness);
    let earlier = operator.reset_admin(x.id).await.expect("first reset");
    let latest = operator.reset_admin(x.id).await.expect("second reset");

    // Act
    let old_identity = sign_in(&rig.world, OIDC, "x-old", None, |_| {}).await;
    let with_earlier = sign_in(&rig.world, OIDC, "y-1", Some(earlier.0.expose()), |_| {}).await;
    let with_latest = sign_in(&rig.world, OIDC, "y-2", Some(latest.0.expose()), |_| {}).await;

    // Assert
    assert!(
        matches!(old_identity, Err(SignInError::InvitationRequired)),
        "{old_identity:?}"
    );
    assert!(
        matches!(with_earlier, Err(SignInError::InvitationRequired)),
        "{with_earlier:?}"
    );
    assert!(
        matches!(with_latest, Ok(SignInOutcome::Session { .. })),
        "{with_latest:?}"
    );
    assert_eq!(
        identities_of(&mut rig.world.conn, x.id).await,
        vec![(OIDC.to_owned(), "y-2".to_owned())]
    );
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn the_only_active_admin_can_be_reset_and_stays_an_active_admin() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let x = admin_x(&mut rig).await;
    let operator = invites(&rig.world.harness);

    // Act
    let result = operator.reset_admin(x.id).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        role_and_state(&mut rig.world.conn, x.id).await,
        ("admin".to_owned(), "active".to_owned())
    );
    assert_eq!(live_credentials(&mut rig.world.conn, x.id).await, (0, 0));
    let rows = audit::of(&mut rig.world.conn, "admin_action").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].actor_account_id, None);
    assert_eq!(rows[0].subject_id, Some(x.id));
    let detail = rows[0].detail.clone().unwrap_or_default();
    assert!(detail.contains(r#""action": "admin_reset""#), "{detail}");
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn a_reset_of_an_unknown_or_a_member_account_is_refused_and_changes_nothing() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    rig.member_invite(INVITE).await;
    sign_in(&rig.world, OIDC, "member", Some(INVITE), |_| {})
        .await
        .expect("a member");
    let member = rig.account_of(OIDC, "member").await.expect("member").0;
    let before = (
        token_rows(&mut rig.world.conn).await,
        live_credentials(&mut rig.world.conn, member).await,
    );
    let operator = invites(&rig.world.harness);

    // Act
    let unknown = operator.reset_admin(Uuid::from_u128(7)).await;
    let not_admin = operator.reset_admin(member).await;

    // Assert
    assert!(
        matches!(unknown, Err(OperatorIdentityError::NotFound)),
        "{unknown:?}"
    );
    assert!(
        matches!(not_admin, Err(OperatorIdentityError::NotAnAdmin)),
        "{not_admin:?}"
    );
    let after = (
        token_rows(&mut rig.world.conn).await,
        live_credentials(&mut rig.world.conn, member).await,
    );
    assert_eq!(after, before);
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn a_failed_admin_reset_audit_write_fails_the_reset_and_changes_nothing() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let x = admin_x(&mut rig).await;
    let before = (
        token_rows(&mut rig.world.conn).await,
        live_credentials(&mut rig.world.conn, x.id).await,
        identities_of(&mut rig.world.conn, x.id).await,
    );
    audit::block_inserts(&mut rig.world.conn).await;
    let operator = invites(&rig.world.harness);

    // Act
    let result = operator.reset_admin(x.id).await;

    // Assert
    assert!(result.is_err(), "{result:?}");
    let after = (
        token_rows(&mut rig.world.conn).await,
        live_credentials(&mut rig.world.conn, x.id).await,
        identities_of(&mut rig.world.conn, x.id).await,
    );
    assert_eq!(after, before);
    drop(operator);
    rig.finish().await;
}

#[tokio::test]
async fn a_rebind_writes_the_identity_rebound_event_and_calls_no_hook() {
    // Arrange
    let hook = support::registration::FaultyHook::counting();
    let mut rig = Rig::new(POOL, |_| {}, vec![hook.clone()]).await;
    let x = admin_x(&mut rig).await.id;
    let reset = invites(&rig.world.harness)
        .reset_admin(x)
        .await
        .expect("reset");
    let calls_before = hook.calls();

    // Act
    let outcome = sign_in(&rig.world, OIDC, "y-new", Some(reset.0.expose()), |_| {}).await;

    // Assert
    assert!(
        matches!(outcome, Ok(SignInOutcome::Session { .. })),
        "{outcome:?}"
    );
    assert_eq!(hook.calls(), calls_before);
    let rebound: Vec<_> = audit::of(&mut rig.world.conn, "sign_in")
        .await
        .into_iter()
        .filter(|row| {
            row.detail
                .as_deref()
                .is_some_and(|detail| detail.contains("identity_rebound"))
        })
        .collect();
    assert_eq!(rebound.len(), 1);
    assert_eq!(rebound[0].actor_account_id, Some(x));
    rig.finish().await;
}
