//! AC-2: the data-model constraints hold. Each fixture insert that breaks a rule fails with the SQLSTATE and
//! the constraint of that rule, as the schema owner (who bypasses row-level security, so only the constraint
//! can refuse the row).

mod support;

use sqlx::PgConnection;
use support::fixtures::{self, violation};
use support::{Harness, START};
use uuid::Uuid;

const UNIQUE: &str = "23505";
const CHECK: &str = "23514";

struct Fixture {
    harness: Harness,
    conn: PgConnection,
    account: Uuid,
    at: i64,
}

impl Fixture {
    async fn new() -> Fixture {
        let harness = Harness::new().await;
        let mut conn = harness.fixture_conn().await;
        let account = Uuid::new_v4();
        let at = START.as_unix_micros();
        fixtures::account(&mut conn, account, at).await;
        Fixture {
            harness,
            conn,
            account,
            at,
        }
    }

    async fn finish(self) {
        drop(self.conn);
        self.harness.finish().await;
    }
}

fn expected(code: &str, constraint: &str) -> (String, String) {
    (code.to_owned(), constraint.to_owned())
}

#[tokio::test]
async fn second_identity_with_the_same_provider_user_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::identity(&mut f.conn, f.account, "u1", f.at)
        .await
        .expect("first identity");

    // Act
    let result = fixtures::identity(&mut f.conn, f.account, "u1", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "identities_provider_provider_user_id_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn second_session_with_the_same_token_hash_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::session(&mut f.conn, f.account, "t", "r1", f.at)
        .await
        .expect("first session");

    // Act
    let result = fixtures::session(&mut f.conn, f.account, "t", "r2", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "sessions_token_hash_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn second_session_with_the_same_refresh_hash_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::session(&mut f.conn, f.account, "t1", "r", f.at)
        .await
        .expect("first session");

    // Act
    let result = fixtures::session(&mut f.conn, f.account, "t2", "r", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "sessions_refresh_hash_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn second_device_token_with_the_same_token_hash_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::device_token(&mut f.conn, f.account, "d", f.at)
        .await
        .expect("first device token");

    // Act
    let result = fixtures::device_token(&mut f.conn, f.account, "d", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "device_tokens_token_hash_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn second_device_grant_with_the_same_device_code_hash_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::device_grant(&mut f.conn, None, "dc", "AAAA-1111", "pending", f.at)
        .await
        .expect("first grant");

    // Act
    let result =
        fixtures::device_grant(&mut f.conn, None, "dc", "BBBB-2222", "pending", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "device_grants_device_code_hash_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn second_pending_grant_with_the_same_user_code_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::device_grant(&mut f.conn, None, "dc1", "AAAA-1111", "pending", f.at)
        .await
        .expect("first grant");

    // Act
    let result =
        fixtures::device_grant(&mut f.conn, None, "dc2", "AAAA-1111", "pending", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "device_grants_user_code_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn second_open_setup_token_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;
    fixtures::invite(&mut f.conn, "setup", "s1", "admin", None, None, f.at)
        .await
        .expect("first setup token");

    // Act
    let result = fixtures::invite(&mut f.conn, "setup", "s2", "admin", None, None, f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(UNIQUE, "invites_open_setup_key")
    );
    f.finish().await;
}

#[tokio::test]
async fn setup_row_granting_user_role_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;

    // Act
    let result = fixtures::invite(&mut f.conn, "setup", "s", "user", None, None, f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(CHECK, "invites_setup_role_check")
    );
    f.finish().await;
}

#[tokio::test]
async fn setup_row_with_a_target_account_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;

    // Act
    let result = fixtures::invite(
        &mut f.conn,
        "setup",
        "s",
        "admin",
        Some(f.account),
        None,
        f.at,
    )
    .await;

    // Assert
    assert_eq!(
        violation(result),
        expected(CHECK, "invites_target_account_id_check")
    );
    f.finish().await;
}

#[tokio::test]
async fn setup_row_with_a_creator_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;

    // Act
    let result = fixtures::invite(
        &mut f.conn,
        "setup",
        "s",
        "admin",
        None,
        Some(f.account),
        f.at,
    )
    .await;

    // Assert
    assert_eq!(
        violation(result),
        expected(CHECK, "invites_created_by_check")
    );
    f.finish().await;
}

#[tokio::test]
async fn relative_return_to_without_a_leading_slash_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;

    // Act
    let result = fixtures::oauth_state(&mut f.conn, "st", "maps", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(CHECK, "oauth_states_return_to_check")
    );
    f.finish().await;
}

#[tokio::test]
async fn protocol_relative_return_to_is_refused() {
    // Arrange
    let mut f = Fixture::new().await;

    // Act
    let result = fixtures::oauth_state(&mut f.conn, "st", "//evil.test/x", f.at).await;

    // Assert
    assert_eq!(
        violation(result),
        expected(CHECK, "oauth_states_return_to_check")
    );
    f.finish().await;
}

#[tokio::test]
async fn oauth_state_without_code_challenge_is_accepted() {
    // Arrange
    let mut f = Fixture::new().await;

    // Act
    let result = fixtures::oauth_state(&mut f.conn, "st", "/maps", f.at).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    f.finish().await;
}
