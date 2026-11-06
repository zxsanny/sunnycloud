//! AC-1 to AC-4 of the session service (IT-01-10): rotation, four concurrent refreshes, replay after the grace
//! and dead refresh tokens. Real PostgreSQL, the manual clock for the grace, the component on role `sunnycloud`.

mod support;

use std::time::Duration;

use contracts::{Authenticator, AuthenticatorError, SessionError, SessionService, SessionTokens};
use sqlx::{Connection, PgConnection};
use support::auth::{FAR_FUTURE, authenticator, session};
use support::fixtures;
use support::sessions::{
    all_rows, is_current_refresh, is_previous_refresh, refresh_token, revocation, sessions,
    shared_sessions,
};
use support::{Harness, START};
use uuid::Uuid;

const POLL: Duration = Duration::from_millis(20);
const REVOCATION_BOUND: Duration = Duration::from_secs(5);
const PARALLEL: usize = 4;

/// One account with one live session: access token `a`, refresh token `a-refresh`.
struct World {
    harness: Harness,
    account: Uuid,
    session_id: Uuid,
    conn: PgConnection,
}

async fn world() -> World {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let account = Uuid::new_v4();
    fixtures::account(&mut conn, account, at).await;
    let session_id =
        fixtures::live_session(&mut conn, account, "a", "Firefox", FAR_FUTURE, at).await;
    World {
        harness,
        account,
        session_id,
        conn,
    }
}

impl World {
    async fn finish(self) {
        let World { harness, conn, .. } = self;
        conn.close().await.expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }
}

fn is_unauthenticated(result: &Result<SessionTokens, SessionError>) -> bool {
    matches!(result, Err(SessionError::Unauthenticated))
}

#[tokio::test]
async fn rotation_issues_a_pair_that_authenticates_and_rotates_again() {
    // Arrange
    let mut world = world().await;
    let service = sessions(&world.harness);
    let authenticator = authenticator(&world.harness);

    // Act
    let tokens = service
        .refresh(&refresh_token("a-refresh"))
        .await
        .expect("refresh");
    let principal = authenticator
        .authenticate(session(tokens.access.expose()))
        .await
        .expect("new access token");
    let second = service.refresh(&tokens.refresh).await;

    // Assert
    assert_eq!(principal.account_id, world.account);
    assert_eq!(principal.session_or_device_id, world.session_id);
    assert_eq!(tokens.expires_in, world.harness.config.access_token_ttl);
    assert!(second.is_ok(), "the new refresh token rotates again");
    assert!(
        is_previous_refresh(&mut world.conn, world.session_id, tokens.refresh.expose()).await,
        "after the second rotation the first successor is the previous token"
    );
    drop(service);
    drop(authenticator);
    world.finish().await;
}

#[tokio::test]
async fn rotation_stores_the_old_refresh_token_as_previous_and_the_new_one_as_current() {
    // Arrange
    let mut world = world().await;
    let service = sessions(&world.harness);

    // Act
    let tokens = service
        .refresh(&refresh_token("a-refresh"))
        .await
        .expect("refresh");

    // Assert
    let id = world.session_id;
    assert!(is_previous_refresh(&mut world.conn, id, "a-refresh").await);
    assert!(is_current_refresh(&mut world.conn, id, tokens.refresh.expose()).await);
    assert_eq!(revocation(&mut world.conn, id).await, (false, None));
    drop(service);
    world.finish().await;
}

#[tokio::test]
async fn rotation_replaces_the_access_token_of_the_session() {
    // Arrange
    let world = world().await;
    let service = sessions(&world.harness);
    let authenticator = authenticator(&world.harness);

    // Act
    service
        .refresh(&refresh_token("a-refresh"))
        .await
        .expect("refresh");
    let old_access = authenticator.authenticate(session("a")).await;

    // Assert
    assert!(matches!(
        old_access,
        Err(AuthenticatorError::Unauthenticated)
    ));
    drop(service);
    drop(authenticator);
    world.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_parallel_refreshes_give_one_successor_and_no_revocation() {
    // Arrange
    let mut world = world().await;
    let service = shared_sessions(&world.harness);
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(PARALLEL));

    // Act
    let mut calls = Vec::new();
    for _ in 0..PARALLEL {
        let (service, barrier) = (service.clone(), barrier.clone());
        calls.push(tokio::spawn(async move {
            barrier.wait().await;
            service.refresh(&refresh_token("a-refresh")).await
        }));
    }
    let mut results = Vec::new();
    for call in calls {
        results.push(call.await.expect("refresh task"));
    }

    // Assert
    let successors: Vec<&SessionTokens> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    let rejected = results.iter().filter(|r| is_unauthenticated(r)).count();
    assert_eq!(successors.len(), 1, "exactly one successor: {results:?}");
    assert_eq!(rejected, PARALLEL - 1, "{results:?}");
    let id = world.session_id;
    assert!(is_current_refresh(&mut world.conn, id, successors[0].refresh.expose()).await);
    assert!(is_previous_refresh(&mut world.conn, id, "a-refresh").await);
    assert_eq!(revocation(&mut world.conn, id).await, (false, None));
    let authenticator = authenticator(&world.harness);
    assert!(
        authenticator
            .authenticate(session(successors[0].access.expose()))
            .await
            .is_ok()
    );
    drop(service);
    drop(authenticator);
    world.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_after_the_grace_revokes_the_session_and_every_token_stops_authenticating() {
    // Arrange
    let mut world = world().await;
    let service = shared_sessions(&world.harness);
    let authenticator = authenticator(&world.harness);
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(PARALLEL));
    let mut calls = Vec::new();
    for _ in 0..PARALLEL {
        let (service, barrier) = (service.clone(), barrier.clone());
        calls.push(tokio::spawn(async move {
            barrier.wait().await;
            service.refresh(&refresh_token("a-refresh")).await
        }));
    }
    let mut successor = None;
    for call in calls {
        if let Ok(tokens) = call.await.expect("refresh task") {
            successor = Some(tokens);
        }
    }
    let successor = successor.expect("one successor");
    authenticator
        .authenticate(session(successor.access.expose()))
        .await
        .expect("warm the cache");
    world.harness.clock.tick(Duration::from_millis(400));

    // Act
    let replay = service.refresh(&refresh_token("a-refresh")).await;
    let mut waited = Duration::ZERO;
    while authenticator
        .authenticate(session(successor.access.expose()))
        .await
        .is_ok()
    {
        world.harness.clock.tick(POLL);
        waited += POLL;
        assert!(
            waited <= REVOCATION_BOUND,
            "still authenticates after {waited:?}"
        );
    }

    // Assert
    assert!(
        matches!(replay, Err(SessionError::ReuseDetected)),
        "{replay:?}"
    );
    assert_eq!(
        revocation(&mut world.conn, world.session_id).await,
        (true, Some("refresh_reuse".to_owned()))
    );
    let next = service.refresh(&successor.refresh).await;
    assert!(
        is_unauthenticated(&next),
        "the successor is dead too: {next:?}"
    );
    drop(service);
    drop(authenticator);
    world.finish().await;
}

#[tokio::test]
async fn previous_token_inside_the_grace_changes_no_row() {
    // Arrange
    let mut world = world().await;
    let service = sessions(&world.harness);
    service
        .refresh(&refresh_token("a-refresh"))
        .await
        .expect("refresh");
    world.harness.clock.tick(Duration::from_millis(100));
    let before = all_rows(&mut world.conn).await;

    // Act
    let replay = service.refresh(&refresh_token("a-refresh")).await;

    // Assert
    assert!(is_unauthenticated(&replay), "{replay:?}");
    assert_eq!(all_rows(&mut world.conn).await, before);
    drop(service);
    world.finish().await;
}

#[tokio::test]
async fn dead_refresh_tokens_are_unauthenticated_and_change_no_row() {
    // Arrange
    let mut world = world().await;
    let at = START.as_unix_micros();
    let short_lived = at + 1_000_000;
    fixtures::live_session(
        &mut world.conn,
        world.account,
        "e",
        "expiring",
        short_lived,
        at,
    )
    .await;
    let service = sessions(&world.harness);
    let first = service
        .refresh(&refresh_token("a-refresh"))
        .await
        .expect("first rotation");
    service
        .refresh(&first.refresh)
        .await
        .expect("second rotation");
    world.harness.clock.tick(Duration::from_secs(2));
    let before = all_rows(&mut world.conn).await;

    // Act
    let older_generation = service.refresh(&refresh_token("a-refresh")).await;
    let expired = service.refresh(&refresh_token("e-refresh")).await;
    let random = service.refresh(&refresh_token("never-issued")).await;

    // Assert
    assert!(
        is_unauthenticated(&older_generation),
        "{older_generation:?}"
    );
    assert!(is_unauthenticated(&expired), "{expired:?}");
    assert!(is_unauthenticated(&random), "{random:?}");
    assert_eq!(all_rows(&mut world.conn).await, before);
    drop(service);
    world.finish().await;
}

#[tokio::test]
async fn revoked_session_refuses_both_its_refresh_generations() {
    // Arrange
    let mut world = world().await;
    let service = sessions(&world.harness);
    let first = service
        .refresh(&refresh_token("a-refresh"))
        .await
        .expect("rotation");
    fixtures::revoke_session(&mut world.conn, world.session_id, START.as_unix_micros()).await;
    let before = all_rows(&mut world.conn).await;

    // Act
    let current = service.refresh(&first.refresh).await;
    let previous = service.refresh(&refresh_token("a-refresh")).await;

    // Assert
    assert!(is_unauthenticated(&current), "{current:?}");
    assert!(is_unauthenticated(&previous), "{previous:?}");
    assert_eq!(all_rows(&mut world.conn).await, before);
    drop(service);
    world.finish().await;
}
