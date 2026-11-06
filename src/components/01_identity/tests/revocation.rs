//! AC-3 (IT-01-2): a revocation is seen by `revalidate` within `session_cache_ttl` plus 50 ms, and
//! `authenticate` then fails. Time is the manual clock, advanced 20 ms per poll; the revocation is a fixture
//! write on the schema owner's connection.

mod support;

use std::time::Duration;

use contracts::{
    Authenticator, AuthenticatorError, Principal, PrincipalKind, PrincipalRevalidator, Role,
};
use sqlx::{Connection, PgConnection};
use support::auth::{FAR_FUTURE, authenticator, device, session};
use support::fixtures;
use support::{Harness, START};
use uuid::Uuid;

const POLL: Duration = Duration::from_millis(20);
const MARGIN: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug)]
enum Path {
    SessionRevoked,
    DeviceRevoked,
    AccountDisabled,
    AccountDeleting,
}

async fn revoke(conn: &mut PgConnection, path: Path, principal: &Principal) {
    let at = START.as_unix_micros();
    match path {
        Path::SessionRevoked => {
            fixtures::revoke_session(conn, principal.session_or_device_id, at).await;
        }
        Path::DeviceRevoked => {
            fixtures::revoke_device(conn, principal.session_or_device_id, at).await;
        }
        Path::AccountDisabled => {
            fixtures::set_account_state(conn, principal.account_id, "disabled").await;
        }
        Path::AccountDeleting => {
            fixtures::set_account_state(conn, principal.account_id, "deleting").await;
        }
    }
}

async fn flips_within_the_bound(path: Path, device_credential: bool) {
    // Arrange
    let harness = Harness::new().await;
    let ttl = harness.config.session_cache_ttl;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let account = Uuid::new_v4();
    fixtures::account(&mut conn, account, at).await;
    fixtures::live_session(&mut conn, account, "s", "browser", FAR_FUTURE, at).await;
    fixtures::named_device(&mut conn, account, "d", "laptop", at).await;
    let authenticator = authenticator(&harness);
    let credential = || {
        if device_credential {
            device("d")
        } else {
            session("s")
        }
    };
    let principal = authenticator
        .authenticate(credential())
        .await
        .expect("live");
    let before = authenticator
        .revalidate(&principal)
        .await
        .expect("revalidate");

    // Act
    revoke(&mut conn, path, &principal).await;
    let mut waited = Duration::ZERO;
    while authenticator
        .revalidate(&principal)
        .await
        .expect("revalidate")
    {
        harness.clock.tick(POLL);
        waited += POLL;
        assert!(
            waited <= ttl + MARGIN,
            "{path:?}: still valid after {waited:?}"
        );
    }
    let after = authenticator.authenticate(credential()).await;

    // Assert
    assert!(before, "{path:?}");
    assert!(
        matches!(after, Err(AuthenticatorError::Unauthenticated)),
        "{path:?}: {after:?}"
    );
    conn.close().await.expect("close fixture connection");
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn revoked_session_is_seen_within_the_cache_ttl() {
    flips_within_the_bound(Path::SessionRevoked, false).await;
}

#[tokio::test]
async fn revoked_device_token_is_seen_within_the_cache_ttl() {
    flips_within_the_bound(Path::DeviceRevoked, true).await;
}

#[tokio::test]
async fn disabled_account_is_seen_within_the_cache_ttl() {
    flips_within_the_bound(Path::AccountDisabled, false).await;
}

#[tokio::test]
async fn deleting_account_is_seen_within_the_cache_ttl() {
    flips_within_the_bound(Path::AccountDeleting, true).await;
}

#[tokio::test]
async fn revalidate_is_false_for_a_credential_whose_row_is_gone() {
    // Arrange
    let harness = Harness::new().await;
    let authenticator = authenticator(&harness);
    let unknown = Principal {
        account_id: Uuid::new_v4(),
        kind: PrincipalKind::Session,
        session_or_device_id: Uuid::new_v4(),
        role: Role::User,
        label: "gone".to_owned(),
    };

    // Act
    let alive = authenticator
        .revalidate(&unknown)
        .await
        .expect("revalidate");

    // Assert
    assert!(!alive);
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
