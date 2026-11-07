//! AC-3: the runtime role has DML only and is refused DDL, sees only the scoped account's `jobs` and `derived`
//! rows and none without a scope, and reads the installation-level `worker_status` in any scope. Rows are
//! written as the schema owner; every read is made as `sunnycloud`.
//!
//! The system-scope half of AC-3 (system scope sees all rows) is in `system_scope_repo.rs`, the only file of
//! this crate on `scripts/ci/run-system-allowlist.txt`.

use sc_db::Scope;
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{
    API, DerivedRow, Fixture, JobRow, account_with_revision, blob, derived, job, sqlstate,
    worker_status,
};

const PERMISSION_DENIED: &str = "42501";
const HASH: [u8; 32] = [3; 32];
const TABLES: [&str; 3] = ["jobs", "derived", "worker_status"];

pub struct Accounts {
    pub fixture: Fixture,
    pub a: Uuid,
    pub b: Uuid,
    pub job_a: Uuid,
    pub job_b: Uuid,
}

/// Two accounts, each with one job and one derived row, and two worker status rows.
pub async fn two_accounts() -> Accounts {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let revision_a = account_with_revision(&mut conn, a, &HASH).await;
    let revision_b = account_with_revision(&mut conn, b, &HASH).await;
    let job_a = job(&mut conn, &JobRow::new(a, revision_a, &HASH))
        .await
        .expect("job of a");
    let job_b = job(&mut conn, &JobRow::new(b, revision_b, &HASH))
        .await
        .expect("job of b");
    for account in [a, b] {
        let blob_id = Uuid::new_v4();
        blob(&mut conn, blob_id, account).await;
        derived(
            &mut conn,
            &DerivedRow {
                account,
                content_hash: &HASH,
                kind: "thumb",
                params_version: 1,
                blob: blob_id,
            },
        )
        .await
        .expect("derived row");
    }
    worker_status(&mut conn, "worker-1").await;
    worker_status(&mut conn, "worker-2").await;
    conn.close().await.expect("close owner connection");
    Accounts {
        fixture,
        a,
        b,
        job_a,
        job_b,
    }
}

async fn visible(accounts: &Accounts, scope: Uuid, sql: &'static str) -> Vec<Uuid> {
    accounts
        .fixture
        .api
        .run(Scope::Account(scope), async |tx| {
            sqlx::query_scalar(sql).fetch_all(tx.conn()).await
        })
        .await
        .expect("scoped read")
}

const DERIVED_ACCOUNTS: &str = "SELECT account_id FROM derived ORDER BY account_id";
const JOB_IDS: &str = "SELECT id FROM jobs ORDER BY id";

#[tokio::test]
async fn scoped_to_b_sees_none_of_the_derived_rows_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let seen = visible(&accounts, accounts.b, DERIVED_ACCOUNTS).await;

    // Assert
    assert_eq!(seen, vec![accounts.b]);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_a_sees_only_the_derived_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let seen = visible(&accounts, accounts.a, DERIVED_ACCOUNTS).await;

    // Assert
    assert_eq!(seen, vec![accounts.a]);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_b_sees_only_the_job_of_b_and_cannot_change_the_job_or_derived_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let seen = visible(&accounts, accounts.b, JOB_IDS).await;
    let changed = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            let jobs = sqlx::query("UPDATE jobs SET attempts = attempts + 1 WHERE id = $1")
                .bind(accounts.job_a)
                .execute(tx.conn())
                .await?
                .rows_affected();
            let derived = sqlx::query("UPDATE derived SET size = 1 WHERE account_id = $1")
                .bind(accounts.a)
                .execute(tx.conn())
                .await?
                .rows_affected();
            Ok::<_, sqlx::Error>((jobs, derived))
        })
        .await
        .expect("scoped update");

    // Assert
    assert_eq!(seen, vec![accounts.job_b]);
    assert_eq!(changed, (0, 0));
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_job_and_no_derived_row() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.db.conn_as(API).await;

    // Act
    let jobs: Vec<Uuid> = sqlx::query_scalar(JOB_IDS)
        .fetch_all(&mut conn)
        .await
        .expect("unscoped read");
    let derived: Vec<Uuid> = sqlx::query_scalar(DERIVED_ACCOUNTS)
        .fetch_all(&mut conn)
        .await
        .expect("unscoped read");

    // Assert
    assert!(jobs.is_empty());
    assert!(derived.is_empty());
    conn.close().await.expect("close api connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn worker_status_is_installation_level_and_visible_in_an_account_scope() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;

    // Act
    let seen: Vec<String> = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            sqlx::query_scalar("SELECT worker_id FROM worker_status ORDER BY worker_id")
                .fetch_all(tx.conn())
                .await
        })
        .await
        .expect("scoped read");
    let policies: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_policies WHERE tablename = 'worker_status'")
            .fetch_one(&mut conn)
            .await
            .expect("count policies");
    let rls: bool = sqlx::query_scalar(
        "SELECT relrowsecurity FROM pg_class WHERE oid = 'public.worker_status'::regclass",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read rls flag");

    // Assert
    assert_eq!(seen, ["worker-1", "worker-2"]);
    assert_eq!(policies, 0);
    assert!(!rls);
    conn.close().await.expect("close owner connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn runtime_role_cannot_alter_drop_or_truncate_the_tables() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.db.conn_as(API).await;

    // Act
    let mut results = Vec::new();
    for table in TABLES {
        for statement in [
            format!("ALTER TABLE {table} ADD COLUMN extra bigint"),
            format!("TRUNCATE {table}"),
            format!("DROP TABLE {table}"),
            format!("CREATE INDEX extra_idx ON {table} ((1))"),
        ] {
            let result = sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
                .execute(&mut conn)
                .await;
            results.push((statement, result));
        }
    }

    // Assert
    for (statement, result) in results {
        let error = result.expect_err("DDL is refused");
        assert_eq!(sqlstate(&error), PERMISSION_DENIED, "{statement}");
    }
    conn.close().await.expect("close api connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn runtime_role_has_dml_privileges_only() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let privileges = [
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "TRUNCATE",
        "REFERENCES",
        "TRIGGER",
    ];

    // Act
    let mut granted = Vec::new();
    for table in TABLES {
        let mut table_privileges = Vec::new();
        for privilege in privileges {
            let has: bool = sqlx::query_scalar("SELECT has_table_privilege('sunnycloud', $1, $2)")
                .bind(table)
                .bind(privilege)
                .fetch_one(&mut conn)
                .await
                .expect("check privilege");
            if has {
                table_privileges.push(privilege);
            }
        }
        granted.push((table, table_privileges));
    }

    // Assert
    let dml = vec!["SELECT", "INSERT", "UPDATE", "DELETE"];
    assert_eq!(granted, TABLES.map(|table| (table, dml.clone())));
    conn.close().await.expect("close owner connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn policies_admit_system_scope_next_to_the_account_match() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;

    // Act
    let policies: Vec<(String, String)> = sqlx::query_as(
        "SELECT tablename::text, qual FROM pg_policies WHERE schemaname = 'public' \
         AND tablename IN ('jobs', 'derived') ORDER BY tablename",
    )
    .fetch_all(&mut conn)
    .await
    .expect("read the policies");

    // Assert
    assert_eq!(policies.len(), 2);
    for (table, qual) in policies {
        assert!(qual.contains("'system'"), "{table}: {qual}");
    }
    conn.close().await.expect("close owner connection");
    accounts.fixture.finish().await;
}
