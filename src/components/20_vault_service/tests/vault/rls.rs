//! AC-2 (AC-59): the runtime role sees only the scoped account's vault, no vault without a scope, cannot change
//! another account's vault and cannot insert a row for another account, and cannot run DDL. Rows are written as
//! the schema owner; every read is made as `sunnycloud`.

use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use uuid::Uuid;

use crate::harness::{API, Harness, account_with_folder, sqlstate, vault};

const PERMISSION_DENIED: &str = "42501";
const READ: &str = "SELECT account_id FROM vaults ORDER BY account_id";

struct Accounts {
    harness: Harness,
    a: Uuid,
    b: Uuid,
    folder_of_b: Uuid,
}

async fn two_accounts() -> Accounts {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let folder_of_a = account_with_folder(&mut conn, a).await;
    let folder_of_b = account_with_folder(&mut conn, b).await;
    vault(&mut conn, a, folder_of_a).await.expect("vault of a");
    vault(&mut conn, b, folder_of_b).await.expect("vault of b");
    conn.close().await.expect("close fixture connection");
    Accounts {
        harness,
        a,
        b,
        folder_of_b,
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
async fn scoped_to_a_sees_only_the_vault_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.a).await;

    // Assert
    assert_eq!(visible, vec![accounts.a]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_b_sees_only_the_vault_of_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.b).await;

    // Assert
    assert_eq!(visible, vec![accounts.b]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_vault() {
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
async fn scoped_to_b_cannot_change_or_delete_the_vault_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let (updated, deleted) = accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            let updated =
                sqlx::query("UPDATE vaults SET header = '\\x00'::bytea WHERE account_id = $1")
                    .bind(accounts.a)
                    .execute(tx.conn())
                    .await?
                    .rows_affected();
            let deleted = sqlx::query("DELETE FROM vaults WHERE account_id = $1")
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
async fn scoped_to_a_cannot_insert_a_vault_for_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let outcome = accounts
        .harness
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            vault(tx.conn(), accounts.b, accounts.folder_of_b)
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
        "SELECT qual FROM pg_policies WHERE schemaname = 'public' AND tablename = 'vaults'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the vaults policy");

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
        "ALTER TABLE vaults ADD COLUMN extra bigint",
        "CREATE INDEX vaults_extra_idx ON vaults (created_at)",
        "TRUNCATE vaults",
        "DROP TABLE vaults",
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
