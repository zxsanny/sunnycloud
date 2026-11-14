//! `AccountAdmin::profile` (AZ-3819, owner decision 2026-11-14): the caller's display name and linked identities,
//! oldest first, read in the account's own scope on a real PostgreSQL with row-level security on.

mod support;

use contracts::{AccountAdmin, AccountAdminError, IdentityView};
use sqlx::PgConnection;
use support::START;
use support::accounts::{accounts, fixture};
use uuid::Uuid;

const SECOND_MICROS: i64 = 1_000_000;

async fn named_account(conn: &mut PgConnection, id: Uuid, name: &str) {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', $3, 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .bind(name)
    .execute(conn)
    .await
    .expect("insert account");
}

async fn linked(
    conn: &mut PgConnection,
    account: Uuid,
    provider: &str,
    email: Option<&str>,
    at: i64,
) {
    sqlx::query(
        "INSERT INTO identities (id, account_id, provider, provider_user_id, email, email_verified, created_at) \
         VALUES (gen_random_uuid(), $1, $2, gen_random_uuid()::text, $3, $3 IS NOT NULL, \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
    )
    .bind(account)
    .bind(provider)
    .bind(email)
    .bind(at)
    .execute(conn)
    .await
    .expect("insert identity");
}

fn identity(provider: &str, email: Option<&str>) -> IdentityView {
    IdentityView {
        provider: provider.to_owned(),
        email: email.map(str::to_owned),
    }
}

#[tokio::test]
async fn profile_returns_the_accounts_own_name_and_identities_oldest_first() {
    // Arrange
    let mut f = fixture().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let at = START.as_unix_micros();
    named_account(&mut f.conn, a, "Ada").await;
    named_account(&mut f.conn, b, "Bob").await;
    linked(&mut f.conn, a, "github", None, at + 2 * SECOND_MICROS).await;
    linked(&mut f.conn, a, "google", Some("ada@example.test"), at).await;
    linked(&mut f.conn, b, "google", Some("bob@example.test"), at).await;
    let admin = accounts(&f.harness);

    // Act
    let profile_a = admin.profile(a).await.expect("profile of A");
    let profile_b = admin.profile(b).await.expect("profile of B");

    // Assert
    assert_eq!(profile_a.display_name, "Ada");
    assert_eq!(
        profile_a.identities,
        [
            identity("google", Some("ada@example.test")),
            identity("github", None)
        ]
    );
    assert_eq!(profile_b.display_name, "Bob");
    assert_eq!(
        profile_b.identities,
        [identity("google", Some("bob@example.test"))]
    );
    drop(admin);
    f.finish().await;
}

#[tokio::test]
async fn profile_of_an_unknown_account_is_an_unexpected_failure() {
    // Arrange
    let f = fixture().await;
    let admin = accounts(&f.harness);

    // Act
    let result = admin.profile(Uuid::new_v4()).await;

    // Assert
    assert!(
        matches!(result, Err(AccountAdminError::Unexpected(_))),
        "{result:?}"
    );
    drop(admin);
    f.finish().await;
}
