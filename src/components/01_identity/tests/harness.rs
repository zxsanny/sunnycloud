//! AC-6: the harness gives each test its own database cloned from the migrated template, the component on
//! role `sunnycloud`, fixtures on `sunnycloud_migrator`, time from the manual clock, providers pointed at the
//! configured mock-idp URL, and no leaked connections.

mod support;

use std::time::Duration;

use sc_clock::Clock;
use sc_db::{DbError, Scope};
use sqlx::Connection;
use support::fixtures;
use support::{Harness, START, mock_idp_url};
use uuid::Uuid;

async fn api_text(harness: &Harness, statement: &'static str) -> String {
    harness
        .api
        .run(Scope::Account(Uuid::nil()), async |tx| {
            sqlx::query_scalar::<_, String>(statement)
                .fetch_one(tx.conn())
                .await
        })
        .await
        .map_err(|error| error.to_string())
        .expect("api read")
}

#[tokio::test]
async fn two_harnesses_started_together_get_different_databases() {
    // Arrange
    let started = tokio::join!(Harness::new(), Harness::new());

    // Act
    let (first, second) = (started.0.db.name.clone(), started.1.db.name.clone());

    // Assert
    assert_ne!(first, second);
    started.0.finish().await;
    started.1.finish().await;
}

#[tokio::test]
async fn each_harness_component_connects_to_its_own_database() {
    // Arrange
    let (first, second) = tokio::join!(Harness::new(), Harness::new());

    // Act
    let seen = [
        api_text(&first, "SELECT current_database()::text").await,
        api_text(&second, "SELECT current_database()::text").await,
    ];

    // Assert
    assert_eq!(seen, [first.db.name.clone(), second.db.name.clone()]);
    first.finish().await;
    second.finish().await;
}

#[tokio::test]
async fn component_connection_runs_as_role_sunnycloud() {
    // Arrange
    let harness = Harness::new().await;

    // Act
    let user = api_text(&harness, "SELECT current_user::text").await;

    // Assert
    assert_eq!(user, "sunnycloud");
    harness.finish().await;
}

#[tokio::test]
async fn fixture_connection_runs_as_role_sunnycloud_migrator() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&mut conn)
        .await
        .expect("fixture read");

    // Assert
    assert_eq!(user, "sunnycloud_migrator");
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn fixture_timestamps_follow_the_manual_clock() {
    // Arrange
    let harness = Harness::new().await;
    harness.clock.tick(Duration::from_secs(3600));
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();

    // Act
    fixtures::account(&mut conn, account, harness.now_micros()).await;
    let stored: i64 = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM created_at) * 1000000)::bigint FROM accounts WHERE id = $1",
    )
    .bind(account)
    .fetch_one(&mut conn)
    .await
    .expect("read back");

    // Assert
    assert_eq!(stored, START.as_unix_micros() + 3_600_000_000);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn manual_clock_starts_at_the_fixed_instant_and_does_not_move_by_itself() {
    // Arrange
    let harness = Harness::new().await;

    // Act
    let now = harness.clock.now_utc();

    // Assert
    assert_eq!(now, START);
    harness.finish().await;
}

#[tokio::test]
async fn providers_point_at_the_configured_mock_idp_url() {
    // Arrange
    let harness = Harness::new().await;

    // Act
    let issuers: Vec<Option<String>> = harness
        .config
        .providers
        .iter()
        .map(|provider| provider.issuer.clone())
        .collect();

    // Assert
    assert_eq!(issuers, vec![Some(mock_idp_url())]);
    harness.finish().await;
}

#[tokio::test]
async fn harness_section_uses_short_test_durations() {
    // Arrange
    let harness = Harness::new().await;

    // Act
    let cache_ttl = harness.config.session_cache_ttl;

    // Assert
    assert_eq!(cache_ttl, Duration::from_millis(200));
    harness.finish().await;
}

#[tokio::test]
async fn open_connections_return_to_the_baseline_after_finish() {
    // Arrange
    let harness = Harness::new().await;
    harness
        .api
        .run(Scope::Account(Uuid::nil()), async |tx| {
            sqlx::query("SELECT 1").execute(tx.conn()).await?;
            Ok::<_, DbError>(())
        })
        .await
        .expect("use the pool");

    // Act
    let remaining = harness.finish().await;

    // Assert
    assert_eq!(remaining, 0);
}
