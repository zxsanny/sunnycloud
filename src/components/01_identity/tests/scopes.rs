//! AC-4 (IT-01-3, ST-01-4): the scope matrix on principals produced by `authenticate`, and no credential
//! other than a session access token or a device token ever gives a principal.

mod support;

use std::time::Duration;

use contracts::{Authenticator, AuthenticatorError, Principal, Scope};
use sqlx::Connection;
use support::auth::{FAR_FUTURE, authenticator, device, session};
use support::fixtures;
use support::{Harness, START};
use uuid::Uuid;

const SCOPES: [Scope; 4] = [
    Scope::Public,
    Scope::SessionOnly,
    Scope::SessionOrDevice,
    Scope::Admin,
];

async fn seeded() -> Harness {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let (member, admin) = (Uuid::new_v4(), Uuid::new_v4());
    fixtures::account_in(&mut conn, member, "user", "active", at).await;
    fixtures::account_in(&mut conn, admin, "admin", "active", at).await;
    fixtures::live_session(&mut conn, member, "member-s", "browser", FAR_FUTURE, at).await;
    fixtures::live_session(&mut conn, admin, "admin-s", "browser", FAR_FUTURE, at).await;
    fixtures::named_device(&mut conn, admin, "admin-d", "laptop", at).await;
    fixtures::device_grant(
        &mut conn,
        Some(member),
        "device-code",
        "BCDFGHJK",
        "approved",
        at,
    )
    .await
    .expect("insert device grant");
    fixtures::login_code(&mut conn, member, "login-code", at).await;
    conn.close().await.expect("close fixture connection");
    harness
}

fn allowed(authenticator: &impl Authenticator, principal: &Principal) -> [bool; 4] {
    SCOPES.map(|scope| authenticator.require(principal, scope).is_ok())
}

#[tokio::test]
async fn scope_matrix_holds_for_member_admin_and_device_principals() {
    // Arrange
    let harness = seeded().await;
    let authenticator = authenticator(&harness);
    let member = authenticator
        .authenticate(session("member-s"))
        .await
        .expect("member");
    let admin = authenticator
        .authenticate(session("admin-s"))
        .await
        .expect("admin");
    let device = authenticator
        .authenticate(device("admin-d"))
        .await
        .expect("device");

    // Act
    let matrix = [
        allowed(&authenticator, &member),
        allowed(&authenticator, &admin),
        allowed(&authenticator, &device),
    ];

    // Assert
    assert_eq!(
        matrix,
        [
            [true, true, true, false],
            [true, true, true, true],
            [true, false, true, false],
        ]
    );
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn refused_scope_is_forbidden() {
    // Arrange
    let harness = seeded().await;
    let authenticator = authenticator(&harness);
    let device = authenticator
        .authenticate(device("admin-d"))
        .await
        .expect("device");

    // Act
    let result = authenticator.require(&device, Scope::SessionOnly);

    // Assert
    assert!(matches!(result, Err(AuthenticatorError::Forbidden)));
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn foreign_bearers_never_give_a_principal_in_either_variant() {
    // Arrange
    let harness = seeded().await;
    let authenticator = authenticator(&harness);
    let foreign = [
        "wk_worker-bearer",
        "member-s-refresh",
        "login-code",
        "device-code",
    ];

    for token in foreign {
        // Act
        let as_session = authenticator.authenticate(session(token)).await;
        let as_device = authenticator.authenticate(device(token)).await;

        // Assert
        assert!(
            matches!(as_session, Err(AuthenticatorError::Unauthenticated)),
            "{token}"
        );
        assert!(
            matches!(as_device, Err(AuthenticatorError::Unauthenticated)),
            "{token}"
        );
    }
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn session_token_presented_as_a_device_credential_is_refused_even_when_cached() {
    // Arrange
    let harness = seeded().await;
    let authenticator = authenticator(&harness);
    authenticator
        .authenticate(session("member-s"))
        .await
        .expect("warm the cache");
    harness.clock.tick(Duration::from_millis(10));

    // Act
    let as_device = authenticator.authenticate(device("member-s")).await;

    // Assert
    assert!(matches!(
        as_device,
        Err(AuthenticatorError::Unauthenticated)
    ));
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
