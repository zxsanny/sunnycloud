//! IT-12-11 (AZ-3793 AC-6) against the real binary and the shipped migrations: the faults are arranged from
//! outside the binary, so the release build carries no test-only migration source. A failing migration exits 2
//! naming its version and the next run retries from it; `migrate` killed with SIGKILL while a migration waits
//! leaves nothing applied and `serve` refusing to start. The irreversible `down` has no shipped migration to meet
//! and is covered by `sc_db` and by the exit mapping unit test of `migration.rs`.

mod root_support;
mod serve_support;

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use root_support::Setup;
use sc_testdb::TestDb;
use serde_json::Value;
use serve_support::Stack;

const DEADLINE: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(50);
/// The second shipped migration, `create_audit_log_table`: it creates `audit_log`.
const CREATES_AUDIT_LOG: i64 = 20_261_105_100_000;

/// The versions of the `up` files the binary embeds, oldest first.
fn embedded_versions() -> Vec<i64> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
    let mut versions: Vec<i64> = std::fs::read_dir(&dir)
        .expect("the migrations directory is readable")
        .filter_map(|entry| {
            let name = entry
                .expect("migration entry")
                .file_name()
                .to_string_lossy()
                .into_owned();
            name.strip_suffix(".up.sql")?
                .split('_')
                .next()?
                .parse()
                .ok()
        })
        .collect();
    versions.sort_unstable();
    versions
}

/// Only the migrator secret of `db`, as the Compose `migrate` step has it.
fn migrator(db: &TestDb) -> Setup {
    let url = db
        .config()
        .db_migrator_url
        .expect("the test config has a migrator URL")
        .expose()
        .clone();
    let mut setup = Setup::empty();
    setup.secret("db_migrator_url", &url);
    setup
}

fn spawn(setup: &Setup, args: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
        .args(args)
        .env_clear()
        .envs(&setup.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts")
}

struct Run(Output);

impl Run {
    fn code(&self) -> Option<i32> {
        self.0.status.code()
    }

    fn stderr(&self) -> String {
        String::from_utf8(self.0.stderr.clone()).expect("UTF-8")
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.0.stdout)
            .unwrap_or_else(|error| panic!("one JSON object ({error}): {}", self.stderr()))
    }

    fn versions(&self, field: &str) -> Vec<i64> {
        serde_json::from_value(self.json()["data"][field].clone()).expect("a list of versions")
    }
}

async fn run(setup: &Setup, args: &[&str]) -> Run {
    let mut child = spawn(setup, args);
    let started = Instant::now();
    while child.try_wait().expect("the child can be polled").is_none() {
        if started.elapsed() > DEADLINE {
            child.kill().expect("kill the hung binary");
            panic!("the binary did not exit within {DEADLINE:?}");
        }
        tokio::time::sleep(POLL).await;
    }
    Run(child.wait_with_output().expect("collect the output"))
}

async fn applied_count(db: &TestDb) -> i64 {
    db.admin_count("SELECT count(*) FROM _sqlx_migrations")
        .await
}

#[tokio::test]
async fn a_failing_migration_exits_2_naming_its_version_and_the_next_run_retries_from_it() {
    // Arrange: `audit_log` already exists, so the second migration fails on its first statement.
    let db = TestDb::clone_empty().await;
    db.admin_exec("CREATE TABLE audit_log (x int)").await;
    let setup = migrator(&db);
    let all = embedded_versions();
    assert_eq!(all[1], CREATES_AUDIT_LOG);

    // Act
    let failed = run(&setup, &["--json", "migrate"]).await;
    let applied_after_failure = applied_count(&db).await;
    db.admin_exec("DROP TABLE audit_log").await;
    let retried = run(&setup, &["--json", "migrate"]).await;

    // Assert
    assert_eq!(failed.code(), Some(2), "{}", failed.stderr());
    assert_eq!(failed.json()["code"], "migration_failed");
    assert!(
        failed.stderr().contains(&CREATES_AUDIT_LOG.to_string()),
        "{}",
        failed.stderr()
    );
    assert_eq!(
        applied_after_failure, 1,
        "the migration before it stays applied"
    );
    assert_eq!(retried.code(), Some(0), "{}", retried.stderr());
    assert_eq!(retried.versions("applied"), all[1..].to_vec());
    db.drop_db().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn migrate_killed_while_a_migration_waits_leaves_it_unapplied_and_serve_refuses() {
    // Arrange: the newest migration is made pending by the real `revert`; then an ACCESS SHARE lock held on every
    // application table (sqlx's bookkeeping table excepted) keeps the next `migrate` waiting inside it, whatever
    // table the newest migration alters.
    let stack = Stack::new().await;
    let db = &stack.root.db;
    let setup = migrator(db);
    let newest = *embedded_versions().last().expect("one migration");
    let reverted = run(&setup, &["migrate", "revert", "--steps", "1", "--yes"]).await;
    assert_eq!(reverted.code(), Some(0), "{}", reverted.stderr());
    let pending_schema = schema_fingerprint(db).await;
    let mut holder = db.admin().await;
    sqlx::query("BEGIN")
        .execute(&mut holder)
        .await
        .expect("begin");
    let tables: String = sqlx::query_scalar(APPLICATION_TABLES)
        .fetch_one(&mut holder)
        .await
        .expect("list the application tables");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "LOCK TABLE {tables} IN ACCESS SHARE MODE"
    )))
    .execute(&mut holder)
    .await
    .expect("lock the application tables");
    let mut child = spawn(&setup, &["migrate"]);
    let started = Instant::now();
    while db
        .admin_count("SELECT count(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock'")
        .await
        == 0
    {
        assert!(
            started.elapsed() < DEADLINE,
            "the migration never started waiting"
        );
        tokio::time::sleep(POLL).await;
    }

    // Act
    child.kill().expect("SIGKILL the binary");
    child.wait().expect("reap the binary");
    sqlx::query("ROLLBACK")
        .execute(&mut holder)
        .await
        .expect("release the lock");
    while db
        .admin_count(
            "SELECT count(*) FROM pg_stat_activity WHERE usename = 'sunnycloud_migrator' \
             AND pid <> pg_backend_pid()",
        )
        .await
        > 0
    {
        assert!(
            started.elapsed() < DEADLINE,
            "the killed migration's backend never ended"
        );
        tokio::time::sleep(POLL).await;
    }
    let killed_schema = schema_fingerprint(db).await;
    let status = run(&setup, &["--json", "migrate", "status"]).await;
    let refused = stack.refused();
    let retried = run(&setup, &["--json", "migrate"]).await;

    // Assert
    assert_eq!(
        killed_schema, pending_schema,
        "the killed migration left part of its change behind"
    );
    assert_eq!(status.code(), Some(0), "{}", status.stderr());
    assert_eq!(status.versions("pending"), [newest]);
    assert_eq!(
        refused.exited.code,
        Some(2),
        "{}",
        refused.exited.stderr_text()
    );
    assert!(!refused.accepted, "a listener accepted before the refusal");
    assert!(
        refused.exited.stderr_text().contains("pending"),
        "{}",
        refused.exited.stderr_text()
    );
    assert_eq!(retried.code(), Some(0), "{}", retried.stderr());
    assert_eq!(retried.versions("applied"), [newest]);
    drop(holder);
    stack.finish().await;
}

/// Every application table of the public schema, comma-separated and quoted, for one `LOCK TABLE`.
const APPLICATION_TABLES: &str = "SELECT string_agg(format('%I.%I', schemaname, tablename), ', ' \
     ORDER BY tablename) FROM pg_tables WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'";

/// The columns and constraints of the public schema as one text: equal before and after means a migration left
/// nothing of itself behind.
async fn schema_fingerprint(db: &TestDb) -> String {
    let mut conn = db.admin().await;
    let fingerprint: String = sqlx::query_scalar(
        "SELECT coalesce((SELECT string_agg(table_name || '.' || column_name || ':' || data_type || ':' \
         || is_nullable, ',' ORDER BY table_name, column_name) FROM information_schema.columns \
         WHERE table_schema = 'public'), '') || '|' || coalesce((SELECT string_agg(conname || ':' \
         || pg_get_constraintdef(oid), ',' ORDER BY conname) FROM pg_constraint \
         WHERE connamespace = 'public'::regnamespace), '')",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the schema");
    sqlx::Connection::close(conn).await.expect("close");
    fingerprint
}
