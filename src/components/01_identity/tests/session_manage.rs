//! AC-5 to AC-7 of the session service (IT-01-2 rows, ST-01-1): revocation reaching `revalidate`, the account
//! boundary of `list` and `revoke`, and the once-a-minute `last_seen_at` write. Real PostgreSQL, manual clock.

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use contracts::{
    Authenticator, AuthenticatorError, PrincipalKind, PrincipalRevalidator, RevokeReason,
    SessionService,
};
use sqlx::{Connection, PgConnection};
use support::auth::{FAR_FUTURE, authenticator, session};
use support::fixtures;
use support::sessions::{
    device_last_seen, principal, revocation, row_version, session_last_seen, sessions,
};
use support::{Harness, START};
use uuid::Uuid;

const POLL: Duration = Duration::from_millis(20);
const MARGIN: Duration = Duration::from_millis(50);

struct Accounts {
    harness: Harness,
    conn: PgConnection,
    a: Uuid,
    b: Uuid,
    a_sessions: [Uuid; 2],
    b_sessions: [Uuid; 2],
}

/// Accounts A and B with two live sessions each; access tokens `a1`, `a2`, `b1`, `b2`.
async fn accounts() -> Accounts {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    fixtures::account(&mut conn, a, at).await;
    fixtures::account(&mut conn, b, at).await;
    let mut ids = Vec::new();
    for (account, token) in [(a, "a1"), (a, "a2"), (b, "b1"), (b, "b2")] {
        ids.push(fixtures::live_session(&mut conn, account, token, token, FAR_FUTURE, at).await);
    }
    Accounts {
        harness,
        conn,
        a,
        b,
        a_sessions: [ids[0], ids[1]],
        b_sessions: [ids[2], ids[3]],
    }
}

impl Accounts {
    async fn finish(self) {
        let Accounts { harness, conn, .. } = self;
        conn.close().await.expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }
}

#[tokio::test]
async fn revoke_and_revoke_all_turn_revalidate_false_within_the_cache_ttl() {
    // Arrange
    let accounts = accounts().await;
    let service = sessions(&accounts.harness);
    let authenticator = authenticator(&accounts.harness);
    let bound = accounts.harness.config.session_cache_ttl + MARGIN;
    let by_revoke = authenticator.authenticate(session("a1")).await.expect("a1");
    let by_revoke_all = authenticator.authenticate(session("b1")).await.expect("b1");
    assert!(
        authenticator
            .revalidate(&by_revoke)
            .await
            .expect("revalidate")
    );
    assert!(
        authenticator
            .revalidate(&by_revoke_all)
            .await
            .expect("revalidate")
    );

    // Act
    service
        .revoke(accounts.a, accounts.a_sessions[0], RevokeReason::User)
        .await
        .expect("revoke");
    service
        .revoke_all(accounts.b, RevokeReason::Admin)
        .await
        .expect("revoke_all");
    let mut waited = Duration::ZERO;
    while authenticator
        .revalidate(&by_revoke)
        .await
        .expect("revalidate")
        || authenticator
            .revalidate(&by_revoke_all)
            .await
            .expect("revalidate")
    {
        accounts.harness.clock.tick(POLL);
        waited += POLL;
        assert!(waited <= bound, "still valid after {waited:?}");
    }

    // Assert
    for token in ["a1", "b1"] {
        let result = authenticator.authenticate(session(token)).await;
        assert!(
            matches!(result, Err(AuthenticatorError::Unauthenticated)),
            "{token}: {result:?}"
        );
    }
    drop(service);
    drop(authenticator);
    accounts.finish().await;
}

#[tokio::test]
async fn an_account_lists_and_revokes_only_its_own_sessions() {
    // Arrange
    let mut accounts = accounts().await;
    let service = sessions(&accounts.harness);
    let authenticator = authenticator(&accounts.harness);

    // Act
    let listed = service.list(accounts.b).await.expect("list");
    service
        .revoke(accounts.b, accounts.a_sessions[0], RevokeReason::User)
        .await
        .expect("revoke of a foreign session changes nothing");
    let a_after = service.list(accounts.a).await.expect("list");

    // Assert
    let listed_ids: BTreeSet<Uuid> = listed.iter().map(|info| info.id).collect();
    assert_eq!(listed_ids, BTreeSet::from(accounts.b_sessions));
    assert_eq!(listed.len(), 2);
    assert_eq!(a_after.len(), 2);
    assert_eq!(
        revocation(&mut accounts.conn, accounts.a_sessions[0]).await,
        (false, None)
    );
    assert!(authenticator.authenticate(session("a1")).await.is_ok());
    drop(service);
    drop(authenticator);
    accounts.finish().await;
}

#[tokio::test]
async fn revoke_all_ends_only_the_callers_sessions_with_the_given_reason() {
    // Arrange
    let mut accounts = accounts().await;
    let service = sessions(&accounts.harness);

    // Act
    service
        .revoke_all(accounts.b, RevokeReason::Admin)
        .await
        .expect("revoke_all");

    // Assert
    for id in accounts.b_sessions {
        assert_eq!(
            revocation(&mut accounts.conn, id).await,
            (true, Some("admin".to_owned()))
        );
    }
    for id in accounts.a_sessions {
        assert_eq!(revocation(&mut accounts.conn, id).await, (false, None));
    }
    assert!(service.list(accounts.b).await.expect("list").is_empty());
    drop(service);
    accounts.finish().await;
}

#[tokio::test]
async fn revoke_keeps_the_first_revocation_and_ignores_an_unknown_session() {
    // Arrange
    let mut accounts = accounts().await;
    let service = sessions(&accounts.harness);
    let id = accounts.a_sessions[1];
    service
        .revoke(accounts.a, id, RevokeReason::User)
        .await
        .expect("first revoke");

    // Act
    let again = service.revoke(accounts.a, id, RevokeReason::Admin).await;
    let unknown = service
        .revoke(accounts.a, Uuid::new_v4(), RevokeReason::User)
        .await;

    // Assert
    assert!(again.is_ok() && unknown.is_ok());
    assert_eq!(
        revocation(&mut accounts.conn, id).await,
        (true, Some("user".to_owned()))
    );
    drop(service);
    accounts.finish().await;
}

#[tokio::test]
async fn touch_writes_last_seen_at_once_per_interval() {
    // Arrange
    let mut accounts = accounts().await;
    let service = sessions(&accounts.harness);
    let interval = accounts.harness.config.session_last_seen_interval;
    let id = accounts.a_sessions[0];
    let who = principal(accounts.a, id, PrincipalKind::Session);
    let start = START.as_unix_micros();

    // Act
    service.touch(&who).await.expect("first touch");
    let first_version = row_version(&mut accounts.conn, id).await;
    for _ in 0..9 {
        accounts.harness.clock.tick(interval / 12);
        service
            .touch(&who)
            .await
            .expect("touch inside the interval");
    }
    let version_inside = row_version(&mut accounts.conn, id).await;
    let seen_inside = session_last_seen(&mut accounts.conn, id).await;
    accounts.harness.clock.tick(interval);
    service.touch(&who).await.expect("touch after the interval");

    // Assert
    assert_eq!(
        version_inside, first_version,
        "ten touches inside one interval write once"
    );
    assert_eq!(seen_inside, Some(start));
    let seen_after = session_last_seen(&mut accounts.conn, id).await;
    assert_eq!(seen_after, Some(accounts.harness.now_micros()));
    assert_ne!(row_version(&mut accounts.conn, id).await, first_version);
    let listed = service.list(accounts.a).await.expect("list");
    let touched = listed.iter().find(|info| info.id == id).expect("listed");
    assert_eq!(
        touched.last_seen_at.as_unix_micros(),
        accounts.harness.now_micros()
    );
    let untouched = listed.iter().find(|info| info.id != id).expect("listed");
    assert_eq!(untouched.last_seen_at, untouched.created_at);
    drop(service);
    accounts.finish().await;
}

#[tokio::test]
async fn touch_does_not_write_a_revoked_session() {
    // Arrange
    let mut accounts = accounts().await;
    let service = sessions(&accounts.harness);
    let id = accounts.a_sessions[0];
    service
        .revoke(accounts.a, id, RevokeReason::User)
        .await
        .expect("revoke");

    // Act
    service
        .touch(&principal(accounts.a, id, PrincipalKind::Session))
        .await
        .expect("touch");

    // Assert
    assert_eq!(session_last_seen(&mut accounts.conn, id).await, None);
    drop(service);
    accounts.finish().await;
}

#[tokio::test]
async fn touch_of_a_device_principal_writes_the_device_token_once_per_interval() {
    // Arrange
    let mut accounts = accounts().await;
    let service = sessions(&accounts.harness);
    let interval = accounts.harness.config.session_last_seen_interval;
    let device = fixtures::named_device(
        &mut accounts.conn,
        accounts.a,
        "d",
        "laptop",
        START.as_unix_micros(),
    )
    .await;
    let who = principal(accounts.a, device, PrincipalKind::Device);

    // Act
    service.touch(&who).await.expect("first touch");
    accounts.harness.clock.tick(interval / 2);
    service
        .touch(&who)
        .await
        .expect("touch inside the interval");
    let inside = device_last_seen(&mut accounts.conn, device).await;
    accounts.harness.clock.tick(interval);
    service.touch(&who).await.expect("touch after the interval");

    // Assert
    assert_eq!(inside, Some(START.as_unix_micros()));
    assert_eq!(
        device_last_seen(&mut accounts.conn, device).await,
        Some(accounts.harness.now_micros())
    );
    drop(service);
    accounts.finish().await;
}
