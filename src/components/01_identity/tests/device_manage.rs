//! AC-5 and AC-6 (IT-01-2 `revoke_device` path, IT-01-15 `revoke_self` part, ST-01-1): device token scope and
//! revocation, and the per-account device list. Real PostgreSQL, manual clock.

mod support;

use std::time::Duration;

use contracts::{
    Authenticator, AuthenticatorError, DeviceGrantError, DeviceGrantService, Principal,
    PrincipalRevalidator, Scope,
};
use sc_ids::RawToken;
use sqlx::Connection;
use sunnycloud_identity::IdentityAuthenticator;
use support::auth::{authenticator, device};
use support::device::{geo, grants, member, mint, revoked_count};
use support::{Harness, START, fixtures};
use uuid::Uuid;

const POLL: Duration = Duration::from_millis(20);
const MARGIN: Duration = Duration::from_millis(50);

async fn device_principal(authenticator: &IdentityAuthenticator, token: &RawToken) -> Principal {
    authenticator
        .authenticate(device(token.expose()))
        .await
        .expect("the minted token authenticates")
}

/// Ticks the clock until `revalidate` is false; fails past the cache TTL plus the margin.
async fn flips_within_the_bound(
    harness: &Harness,
    authenticator: &IdentityAuthenticator,
    p: &Principal,
) {
    let bound = harness.config.session_cache_ttl + MARGIN;
    let mut waited = Duration::ZERO;
    while authenticator.revalidate(p).await.expect("revalidate") {
        harness.clock.tick(POLL);
        waited += POLL;
        assert!(waited <= bound, "still valid after {waited:?}");
    }
}

#[tokio::test]
async fn a_minted_token_is_a_device_principal_refused_on_session_only_scopes() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authenticator = authenticator(&harness);
    let token = mint(&service, &approver).await;

    // Act
    let principal = device_principal(&authenticator, &token.0).await;
    let session_only = authenticator.require(&principal, Scope::SessionOnly);
    let sync_scope = authenticator.require(&principal, Scope::SessionOrDevice);

    // Assert
    assert_eq!(principal.kind, contracts::PrincipalKind::Device);
    assert_eq!(principal.account_id, approver.account_id);
    assert!(matches!(session_only, Err(AuthenticatorError::Forbidden)));
    assert!(sync_scope.is_ok());
    conn.close().await.expect("close fixture connection");
    drop((service, authenticator));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn revoke_device_and_revoke_self_each_end_only_their_own_token() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authenticator = authenticator(&harness);
    let first = device_principal(&authenticator, &mint(&service, &approver).await.0).await;
    let second_token = mint(&service, &approver).await;
    let second = device_principal(&authenticator, &second_token.0).await;
    let third = device_principal(&authenticator, &mint(&service, &approver).await.0).await;

    // Act
    service
        .revoke_device(approver.account_id, first.session_or_device_id)
        .await
        .expect("revoke_device");
    flips_within_the_bound(&harness, &authenticator, &first).await;
    let after_first = (
        authenticator.revalidate(&second).await.expect("revalidate"),
        authenticator.revalidate(&third).await.expect("revalidate"),
        revoked_count(&mut conn).await,
    );
    service.revoke_self(&second).await.expect("revoke_self");
    flips_within_the_bound(&harness, &authenticator, &second).await;
    let after_second = (
        authenticator.revalidate(&third).await.expect("revalidate"),
        revoked_count(&mut conn).await,
    );
    let second_again = service.revoke_self(&second).await;
    let reused = authenticator
        .authenticate(device(second_token.0.expose()))
        .await;

    // Assert
    assert_eq!(
        after_first,
        (true, true, 1),
        "only the first device is revoked"
    );
    assert_eq!(after_second, (true, 2), "only the second device is revoked");
    assert!(
        second_again.is_ok(),
        "revoking a revoked device again is a no-op"
    );
    assert!(matches!(reused, Err(AuthenticatorError::Unauthenticated)));
    let reasons: Vec<String> = sqlx::query_scalar(
        "SELECT revoke_reason FROM device_tokens WHERE revoked_at IS NOT NULL ORDER BY created_at",
    )
    .fetch_all(&mut conn)
    .await
    .expect("revoke reasons");
    assert_eq!(reasons, vec!["user", "user"]);
    conn.close().await.expect("close fixture connection");
    drop((service, authenticator));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn a_session_principal_cannot_revoke_self() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authenticator = authenticator(&harness);
    let token = mint(&service, &approver).await;
    let device = device_principal(&authenticator, &token.0).await;

    // Act
    let refused = service.revoke_self(&approver).await;

    // Assert
    assert!(
        matches!(refused, Err(DeviceGrantError::Forbidden)),
        "{refused:?}"
    );
    assert_eq!(revoked_count(&mut conn).await, 0);
    harness
        .clock
        .tick(harness.config.session_cache_ttl + MARGIN);
    assert!(authenticator.revalidate(&device).await.expect("revalidate"));
    conn.close().await.expect("close fixture connection");
    drop((service, authenticator));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn devices_are_listed_and_revoked_per_account() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let (a, b) = (
        member(&mut conn).await.account_id,
        member(&mut conn).await.account_id,
    );
    let mut a_devices = Vec::new();
    let mut b_devices = Vec::new();
    for (account, devices, names) in [
        (a, &mut a_devices, ["a-laptop", "a-desktop"]),
        (b, &mut b_devices, ["b-laptop", "b-desktop"]),
    ] {
        for name in names {
            devices.push(fixtures::named_device(&mut conn, account, name, name, at).await);
        }
    }
    let service = grants(&mut harness, geo(&[]));
    let authenticator = authenticator(&harness);

    // Act
    let listed = service.list_devices(b).await.expect("list_devices");
    let foreign = service.revoke_device(b, a_devices[0]).await;
    let random = service.revoke_device(b, Uuid::new_v4()).await;
    let own = service.revoke_device(b, b_devices[0]).await;
    let own_again = service.revoke_device(b, b_devices[0]).await;
    let listed_after = service.list_devices(b).await.expect("list_devices");
    let mut a_alive = Vec::new();
    for name in ["a-laptop", "a-desktop"] {
        a_alive.push(authenticator.authenticate(device(name)).await.is_ok());
    }

    // Assert
    let mut ids: Vec<Uuid> = listed.iter().map(|d| d.id).collect();
    ids.sort();
    let mut expected = b_devices.clone();
    expected.sort();
    assert_eq!(ids, expected);
    assert!(
        listed
            .iter()
            .all(|d| d.name.starts_with("b-") && d.platform == "linux")
    );
    assert!(listed.iter().all(|d| d.last_seen_at.is_none()));
    assert!(matches!(foreign, Err(DeviceGrantError::NotFound)));
    assert!(matches!(random, Err(DeviceGrantError::NotFound)));
    assert!(own.is_ok());
    assert!(matches!(own_again, Err(DeviceGrantError::NotFound)));
    assert_eq!(
        listed_after.iter().map(|d| d.id).collect::<Vec<_>>(),
        vec![b_devices[1]]
    );
    assert_eq!(a_alive, vec![true, true]);
    assert_eq!(service.list_devices(a).await.expect("list").len(), 2);
    conn.close().await.expect("close fixture connection");
    drop((service, authenticator));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
