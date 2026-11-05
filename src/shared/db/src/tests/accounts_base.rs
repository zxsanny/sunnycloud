//! AC-1 (first migration), AC-2 (tenant tables reference it) and AC-3 (up, down, up) on a real PostgreSQL 18.

use sqlx::{AssertSqlSafe, PgConnection};
use uuid::Uuid;

use super::support::{API, MIGRATOR, TestDb};
use crate::{Db, DbError, Migrator, RunError, Scope};

pub const BASE_VERSION: i64 = 20_261_105_090_000;

async fn column_rows(conn: &mut PgConnection) -> Vec<(String, String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text, column_default::text \
         FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'accounts' \
         ORDER BY ordinal_position",
    )
    .fetch_all(conn)
    .await
    .expect("read the columns of accounts")
}

async fn scalar<T>(conn: &mut PgConnection, sql: &str) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
{
    sqlx::query_scalar(AssertSqlSafe(sql.to_string()))
        .fetch_one(conn)
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"))
}

fn closure_error<T: std::fmt::Debug>(result: Result<T, RunError<DbError>>) -> DbError {
    match result.expect_err("the statement fails") {
        RunError::Closure(error) => error,
        RunError::Db(error) => panic!("expected the closure's error, got Db({error})"),
    }
}

async fn insert_account(api: &Db, id: Uuid, state: &'static str) -> Result<(), RunError<DbError>> {
    api.run(Scope::Account(id), async |tx| {
        sqlx::query(
            "INSERT INTO accounts (id, state, created_at) VALUES ($1, $2, '2026-11-05T00:00:00Z'::timestamptz)",
        )
        .bind(id)
        .bind(state)
        .execute(tx.conn())
        .await?;
        Ok::<_, DbError>(())
    })
    .await
}

#[tokio::test]
async fn first_migration_creates_accounts_with_owner_grants_and_check() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let api = db.api().await;

    let report = Migrator::run_pending(&migrator)
        .await
        .expect("run_pending on an empty database");

    assert_eq!(report.applied, [BASE_VERSION]);
    let mut admin = db.admin().await;
    assert_eq!(
        column_rows(&mut admin).await,
        [
            ("id".into(), "uuid".into(), "NO".into(), None),
            ("state".into(), "text".into(), "NO".into(), None),
            (
                "created_at".into(),
                "timestamp with time zone".into(),
                "NO".into(),
                None
            ),
        ]
    );
    let owner: String = scalar(
        &mut admin,
        "SELECT pg_get_userbyid(relowner)::text FROM pg_class WHERE oid = 'public.accounts'::regclass",
    )
    .await;
    assert_eq!(owner, MIGRATOR);
    for privilege in ["SELECT", "INSERT", "UPDATE", "DELETE"] {
        let granted: bool = scalar(
            &mut admin,
            &format!("SELECT has_table_privilege('{API}', 'public.accounts', '{privilege}')"),
        )
        .await;
        assert!(granted, "{API} lacks {privilege} on accounts");
    }
    for privilege in ["TRUNCATE", "REFERENCES", "TRIGGER"] {
        let granted: bool = scalar(
            &mut admin,
            &format!("SELECT has_table_privilege('{API}', 'public.accounts', '{privilege}')"),
        )
        .await;
        assert!(!granted, "{API} holds {privilege} on accounts");
    }
    let owned: i64 = scalar(
        &mut admin,
        &format!(
            "SELECT (SELECT count(*) FROM pg_class WHERE relowner = '{API}'::regrole) \
                  + (SELECT count(*) FROM pg_proc WHERE proowner = '{API}'::regrole) \
                  + (SELECT count(*) FROM pg_namespace WHERE nspowner = '{API}'::regrole) \
                  + (SELECT count(*) FROM pg_database WHERE datdba = '{API}'::regrole)"
        ),
    )
    .await;
    assert_eq!(owned, 0, "the API role owns nothing");
    let constraints: Vec<(String, String)> = sqlx::query_as(
        "SELECT conname::text, pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conrelid = 'public.accounts'::regclass AND contype IN ('p', 'c') ORDER BY conname",
    )
    .fetch_all(&mut admin)
    .await
    .expect("read constraints");
    assert_eq!(constraints.len(), 2, "{constraints:?}");
    assert_eq!(constraints[0].0, "accounts_pkey");
    assert_eq!(constraints[1].0, "accounts_state_check");
    for state in ["active", "disabled", "deleting"] {
        insert_account(&api, Uuid::new_v4(), state)
            .await
            .unwrap_or_else(|error| panic!("state {state} is allowed: {error:?}"));
    }
    let archived = insert_account(&api, Uuid::new_v4(), "archived").await;
    assert_eq!(closure_error(archived), DbError::CheckViolation);
    db.drop_db().await;
}

#[tokio::test]
async fn tenant_tables_reference_accounts_with_restrict() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    Migrator::run_pending(&migrator)
        .await
        .expect("run_pending on an empty database");
    migrator
        .run_system("test_tenant_table", async |tx| {
            sqlx::raw_sql(
                "CREATE TABLE tenant_items (id serial PRIMARY KEY, \
                    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE RESTRICT); \
                 GRANT SELECT, INSERT ON tenant_items TO sunnycloud; \
                 GRANT USAGE ON SEQUENCE tenant_items_id_seq TO sunnycloud;",
            )
            .execute(tx.conn())
            .await?;
            Ok::<_, DbError>(())
        })
        .await
        .expect("create the tenant table as the migrator");
    let api = db.api().await;
    let existing = Uuid::new_v4();
    insert_account(&api, existing, "active")
        .await
        .expect("insert the account");
    let add_item = async |account: Uuid| {
        api.run(Scope::Account(account), async |tx| {
            sqlx::query("INSERT INTO tenant_items (account_id) VALUES ($1)")
                .bind(account)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await
    };

    let for_existing = add_item(existing).await;
    let for_missing = add_item(Uuid::new_v4()).await;
    let delete_referenced = api
        .run(Scope::Account(existing), async |tx| {
            sqlx::query("DELETE FROM accounts WHERE id = $1")
                .bind(existing)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await;

    assert!(for_existing.is_ok(), "{for_existing:?}");
    assert_eq!(closure_error(for_missing), DbError::ForeignKeyViolation);
    assert_eq!(
        closure_error(delete_referenced),
        DbError::ForeignKeyViolation
    );
    db.drop_db().await;
}

/// Columns, constraints and owners with their grants: everything the migrations in `public` define.
async fn schema_fingerprint(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT 'column ' || table_name || '.' || column_name || ' ' || data_type || ' ' || is_nullable \
                || ' ' || coalesce(column_default, '-') \
           FROM information_schema.columns WHERE table_schema = 'public' \
         UNION ALL \
         SELECT 'constraint ' || conrelid::regclass::text || ' ' || conname || ' ' || pg_get_constraintdef(oid) \
           FROM pg_constraint WHERE connamespace = 'public'::regnamespace \
         UNION ALL \
         SELECT 'relation ' || relname || ' ' || pg_get_userbyid(relowner) || ' ' || coalesce(relacl::text, '-') \
           FROM pg_class WHERE relnamespace = 'public'::regnamespace AND relkind IN ('r', 'S', 'i') \
         ORDER BY 1",
    )
    .fetch_all(conn)
    .await
    .expect("read the schema")
}

#[tokio::test]
async fn up_down_up_leaves_the_same_schema() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let mut admin = db.admin().await;

    let first = Migrator::run_pending(&migrator).await.expect("first up");
    let after_first = schema_fingerprint(&mut admin).await;
    let down = Migrator::revert(&migrator, u32::MAX).await.expect("down");
    let after_down = schema_fingerprint(&mut admin).await;
    let still_granted: bool = scalar(
        &mut admin,
        &format!("SELECT has_table_privilege('{API}', '_sqlx_migrations', 'SELECT')"),
    )
    .await;
    let second = Migrator::run_pending(&migrator).await.expect("second up");
    let after_second = schema_fingerprint(&mut admin).await;

    assert_eq!(first.applied, [BASE_VERSION]);
    assert_eq!(down.reverted, [BASE_VERSION]);
    assert_eq!(second.applied, [BASE_VERSION]);
    assert!(
        after_first
            .iter()
            .any(|line| line.contains("accounts_state_check"))
    );
    assert!(
        !after_down.iter().any(|line| line.contains("accounts")),
        "{after_down:?}"
    );
    assert!(
        !still_granted,
        "the down revokes the grant on the bookkeeping table"
    );
    assert_eq!(after_first, after_second);
    let status = Migrator::status(&migrator).await.expect("status");
    assert_eq!(status.applied, [BASE_VERSION]);
    db.drop_db().await;
}
