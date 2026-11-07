//! AC-3 (ST-06-8, NFT-SEC-46): the runtime role sees only the scoped account's links, no link without a scope, and
//! cannot run DDL. Rows are written as the schema owner; every read is made as `sunnycloud`. The single
//! system-scope lookup by token hash belongs to the lookup task, whose repository module is the file that needs
//! the `run_system` allowlist line.

use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use uuid::Uuid;

use crate::harness::{API, Harness, account_with_file, link, sqlstate};

const PERMISSION_DENIED: &str = "42501";
const REVOKED_AT_MICROS: i64 = 1_800_000_000_000_000;
const READ: &str = "SELECT account_id FROM share_links ORDER BY token_hash";

struct Accounts {
    harness: Harness,
    a: Uuid,
    b: Uuid,
    file_of_a: Uuid,
}

async fn two_accounts() -> Accounts {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let file_of_a = account_with_file(&mut conn, a).await;
    let file_of_b = account_with_file(&mut conn, b).await;
    link(&mut conn, a, file_of_a, 1).await.expect("link of a");
    link(&mut conn, b, file_of_b, 2).await.expect("link of b");
    conn.close().await.expect("close fixture connection");
    Accounts {
        harness,
        a,
        b,
        file_of_a,
    }
}

async fn visible_to(accounts: &Accounts, scope: Uuid) -> Vec<Uuid> {
    accounts
        .harness
        .api
        .run(Scope::Account(scope), async |tx| {
            sqlx::query_scalar(READ).fetch_all(tx.conn()).await
        })
        .await
        .expect("scoped read")
}

#[tokio::test]
async fn scoped_to_b_sees_only_the_link_of_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.b).await;

    // Assert
    assert_eq!(visible, vec![accounts.b]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_a_sees_only_the_link_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.a).await;

    // Assert
    assert_eq!(visible, vec![accounts.a]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_link() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.harness.db.conn_as(API).await;

    // Act
    let visible: Vec<Uuid> = sqlx::query_scalar(READ)
        .fetch_all(&mut conn)
        .await
        .expect("unscoped read");

    // Assert
    assert!(visible.is_empty());
    conn.close().await.expect("close api connection");
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_b_cannot_change_or_delete_the_link_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let (updated, deleted) = accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            let updated = sqlx::query(
                "UPDATE share_links \
                 SET revoked_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
                 WHERE account_id = $1",
            )
            .bind(accounts.a)
            .bind(REVOKED_AT_MICROS)
            .execute(tx.conn())
            .await?
            .rows_affected();
            let deleted = sqlx::query("DELETE FROM share_links WHERE account_id = $1")
                .bind(accounts.a)
                .execute(tx.conn())
                .await?
                .rows_affected();
            Ok::<_, sqlx::Error>((updated, deleted))
        })
        .await
        .expect("scoped write");

    // Assert
    assert_eq!((updated, deleted), (0, 0));
    assert_eq!(visible_to(&accounts, accounts.a).await, vec![accounts.a]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_b_cannot_insert_a_link_for_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let outcome = accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            link(tx.conn(), accounts.a, accounts.file_of_a, 9)
                .await
                .map_err(DbError::from)
        })
        .await;

    // Assert
    assert!(
        matches!(&outcome, Err(RunError::Closure(DbError::Other(text))) if text.starts_with("SQLSTATE 42501")),
        "{outcome:?}"
    );
    accounts.harness.finish().await;
}

#[tokio::test]
async fn policy_admits_system_scope_next_to_the_account_match() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.harness.fixture_conn().await;

    // Act
    let policy: String = sqlx::query_scalar(
        "SELECT qual FROM pg_policies WHERE schemaname = 'public' AND tablename = 'share_links'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the share_links policy");

    // Assert
    assert!(policy.contains("'system'"), "{policy}");
    conn.close().await.expect("close fixture connection");
    accounts.harness.finish().await;
}

#[tokio::test]
async fn runtime_role_cannot_alter_drop_truncate_or_index_the_table() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.harness.db.conn_as(API).await;
    let statements = [
        "ALTER TABLE share_links ADD COLUMN extra bigint",
        "CREATE INDEX share_links_extra_idx ON share_links (created_at)",
        "TRUNCATE share_links",
        "DROP TABLE share_links",
    ];

    // Act
    let mut states = Vec::new();
    for statement in statements {
        let result = sqlx::raw_sql(statement).execute(&mut conn).await;
        states.push(result.map_or_else(|error| sqlstate(&error), |_| "executed".to_owned()));
    }

    // Assert
    assert_eq!(states, [PERMISSION_DENIED; 4]);
    conn.close().await.expect("close api connection");
    accounts.harness.finish().await;
}
