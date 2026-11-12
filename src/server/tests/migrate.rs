//! IT-12-10 (AZ-3793 AC-1..AC-5): `migrate`, `migrate status` and `migrate revert` of the real binary against a
//! real PostgreSQL 18 database of their own, as the two real roles.

mod config_support;

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use config_support::Setup;
use sc_testdb::TestDb;
use serde_json::Value;

const DEADLINE: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(50);
const FUTURE: i64 = 99_990_101_000_000;

/// The versions of the `up` files the binary embeds, oldest first.
fn embedded_versions() -> Vec<i64> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
    let mut versions: Vec<i64> = std::fs::read_dir(&dir)
        .expect("the migrations directory is readable")
        .map(|entry| entry.expect("migration entry").file_name())
        .filter_map(|name| {
            let name = name.to_string_lossy().into_owned();
            let stem = name.strip_suffix(".up.sql")?;
            stem.split('_').next()?.parse().ok()
        })
        .collect();
    versions.sort_unstable();
    versions
}

/// Only the database secret of one role: no provider, no worker token, no master key.
fn setup_with(db: &TestDb, key: &str) -> Setup {
    let config = db.config();
    let url = match key {
        "db_migrator_url" => config
            .db_migrator_url
            .expect("the test config has a migrator URL")
            .expose()
            .clone(),
        _ => config.db_api_url.expose().clone(),
    };
    let mut setup = Setup::empty();
    setup.secret(key, &url);
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

struct Run {
    output: Output,
}

impl Run {
    fn code(&self) -> Option<i32> {
        self.output.status.code()
    }

    fn stderr(&self) -> String {
        String::from_utf8(self.output.stderr.clone()).expect("UTF-8")
    }

    /// The `--json` envelope.
    fn json(&self) -> Value {
        serde_json::from_slice(&self.output.stdout).unwrap_or_else(|error| {
            panic!("stdout is one JSON object ({error}): {}", self.stderr())
        })
    }

    fn versions(&self, field: &str) -> Vec<i64> {
        serde_json::from_value(self.json()["data"][field].clone()).expect("a list of versions")
    }
}

/// Waits for the child without blocking the runtime; a child past the deadline is killed and fails the test.
async fn finish(mut child: Child) -> Run {
    let started = Instant::now();
    while child.try_wait().expect("the child can be polled").is_none() {
        if started.elapsed() > DEADLINE {
            child.kill().expect("kill the hung binary");
            panic!("the binary did not exit within {DEADLINE:?}");
        }
        tokio::time::sleep(POLL).await;
    }
    Run {
        output: child.wait_with_output().expect("collect the output"),
    }
}

async fn run(setup: &Setup, args: &[&str]) -> Run {
    finish(spawn(setup, args)).await
}

async fn applied_count(db: &TestDb) -> i64 {
    db.admin_count("SELECT count(*) FROM _sqlx_migrations")
        .await
}

#[tokio::test]
async fn a_fresh_database_is_migrated_once_and_the_second_run_applies_nothing() {
    // Arrange
    let db = TestDb::clone_empty().await;
    let setup = setup_with(&db, "db_migrator_url");

    // Act
    let first = run(&setup, &["--json", "migrate"]).await;
    let second = run(&setup, &["--json", "migrate", "run"]).await;

    // Assert
    assert_eq!(first.code(), Some(0), "{}", first.stderr());
    assert_eq!(first.versions("applied"), embedded_versions());
    assert_eq!(second.code(), Some(0), "{}", second.stderr());
    assert!(second.versions("applied").is_empty());
    db.drop_db().await;
}

#[tokio::test]
async fn text_mode_prints_the_applied_versions() {
    // Arrange
    let db = TestDb::clone_empty().await;
    let setup = setup_with(&db, "db_migrator_url");

    // Act
    let result = run(&setup, &["migrate"]).await;

    // Assert
    let stdout = String::from_utf8(result.output.stdout.clone()).expect("UTF-8");
    assert_eq!(result.code(), Some(0), "{}", result.stderr());
    for version in embedded_versions() {
        assert!(stdout.contains(&version.to_string()), "{version}: {stdout}");
    }
    db.drop_db().await;
}

#[tokio::test]
async fn two_concurrent_runs_apply_each_version_exactly_once() {
    // Arrange
    let db = TestDb::clone_empty().await;
    let setup = setup_with(&db, "db_migrator_url");

    // Act
    let (one, two) = (
        spawn(&setup, &["--json", "migrate"]),
        spawn(&setup, &["--json", "migrate"]),
    );
    let (one, two) = (finish(one).await, finish(two).await);

    // Assert
    assert_eq!(one.code(), Some(0), "{}", one.stderr());
    assert_eq!(two.code(), Some(0), "{}", two.stderr());
    let mut both = [one.versions("applied"), two.versions("applied")].concat();
    both.sort_unstable();
    assert_eq!(both, embedded_versions());
    assert_eq!(
        applied_count(&db).await,
        i64::try_from(embedded_versions().len()).expect("count")
    );
    db.drop_db().await;
}

#[tokio::test]
async fn an_edited_checksum_makes_migrate_exit_2_naming_the_version() {
    // Arrange
    let db = TestDb::clone_template().await;
    let newest = *embedded_versions().last().expect("one migration");
    db.admin_exec(&format!(
        "UPDATE _sqlx_migrations SET checksum = '\\xdeadbeef'::bytea WHERE version = {newest}"
    ))
    .await;
    let setup = setup_with(&db, "db_migrator_url");

    // Act
    let migrate = run(&setup, &["--json", "migrate"]).await;
    let status = run(&setup, &["--json", "migrate", "status"]).await;

    // Assert
    assert_eq!(migrate.code(), Some(2), "{}", migrate.stderr());
    assert_eq!(migrate.json()["code"], "checksum_mismatch");
    assert!(
        migrate.stderr().contains(&newest.to_string()),
        "{}",
        migrate.stderr()
    );
    assert_eq!(status.code(), Some(2), "{}", status.stderr());
    assert_eq!(status.versions("checksum-mismatch"), [newest]);
    db.drop_db().await;
}

#[tokio::test]
async fn a_future_version_makes_status_exit_2_reporting_unknown_newer() {
    // Arrange
    let db = TestDb::clone_template().await;
    db.admin_exec(&format!(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES ({FUTURE}, 'future', true, '\\x00'::bytea, 0)"
    ))
    .await;
    let setup = setup_with(&db, "db_migrator_url");

    // Act
    let status = run(&setup, &["--json", "migrate", "status"]).await;
    let migrate = run(&setup, &["--json", "migrate"]).await;

    // Assert
    assert_eq!(status.code(), Some(2), "{}", status.stderr());
    assert_eq!(status.json()["result"], "findings");
    assert_eq!(status.versions("unknown-newer"), [FUTURE]);
    assert_eq!(migrate.code(), Some(2), "{}", migrate.stderr());
    assert_eq!(migrate.json()["code"], "unknown_newer");
    assert!(
        migrate.stderr().contains(&FUTURE.to_string()),
        "{}",
        migrate.stderr()
    );
    db.drop_db().await;
}

#[tokio::test]
async fn an_unsuccessful_row_is_reported_dirty_and_refused_by_migrate() {
    // Arrange
    let db = TestDb::clone_template().await;
    let newest = *embedded_versions().last().expect("one migration");
    db.admin_exec(&format!(
        "UPDATE _sqlx_migrations SET success = false WHERE version = {newest}"
    ))
    .await;
    let setup = setup_with(&db, "db_migrator_url");

    // Act
    let status = run(&setup, &["--json", "migrate", "status"]).await;
    let migrate = run(&setup, &["--json", "migrate"]).await;

    // Assert
    assert_eq!(status.code(), Some(2), "{}", status.stderr());
    assert_eq!(status.json()["data"]["dirty"], newest);
    assert_eq!(migrate.code(), Some(2), "{}", migrate.stderr());
    assert_eq!(migrate.json()["code"], "migration_dirty");
    assert!(
        migrate.stderr().contains(&newest.to_string()),
        "{}",
        migrate.stderr()
    );
    db.drop_db().await;
}

#[tokio::test]
async fn status_works_as_the_api_role_and_lists_applied_and_pending() {
    // Arrange
    let db = TestDb::clone_template().await;
    let setup = setup_with(&db, "db_api_url");

    // Act
    let status = run(&setup, &["--json", "migrate", "status"]).await;
    let migrate = run(&setup, &["migrate"]).await;

    // Assert
    assert_eq!(status.code(), Some(0), "{}", status.stderr());
    assert_eq!(status.versions("applied"), embedded_versions());
    assert!(status.versions("pending").is_empty());
    assert_eq!(status.json()["data"]["dirty"], Value::Null);
    assert_eq!(migrate.code(), Some(3), "{}", migrate.stderr());
    assert!(
        migrate.stderr().contains("db_migrator_url"),
        "{}",
        migrate.stderr()
    );
    db.drop_db().await;
}

#[tokio::test]
async fn revert_without_yes_on_a_non_tty_exits_3_and_with_yes_reverts_one() {
    // Arrange
    let db = TestDb::clone_template().await;
    let setup = setup_with(&db, "db_migrator_url");
    let before = applied_count(&db).await;
    let newest = *embedded_versions().last().expect("one migration");

    // Act
    let refused = run(&setup, &["migrate", "revert", "--steps", "1"]).await;
    let after_refusal = applied_count(&db).await;
    let confirmed = run(
        &setup,
        &["--json", "migrate", "revert", "--steps", "1", "--yes"],
    )
    .await;

    // Assert
    assert_eq!(refused.code(), Some(3), "{}", refused.stderr());
    assert!(refused.output.stdout.is_empty());
    assert_eq!(after_refusal, before);
    assert_eq!(confirmed.code(), Some(0), "{}", confirmed.stderr());
    assert_eq!(confirmed.versions("reverted"), [newest]);
    assert_eq!(applied_count(&db).await, before - 1);
    db.drop_db().await;
}

#[tokio::test]
async fn migrate_needs_no_master_key() {
    // Arrange: the master key secret points at a file that does not exist.
    let db = TestDb::clone_template().await;
    let mut setup = setup_with(&db, "db_migrator_url");
    let missing = setup.dir.join("absent-master.key");
    setup.secret("master_key_file", &missing.display().to_string());

    // Act
    let result = run(&setup, &["--json", "migrate"]).await;

    // Assert
    assert!(!missing.exists());
    assert_eq!(result.code(), Some(0), "{}", result.stderr());
    assert!(result.versions("applied").is_empty());
    db.drop_db().await;
}
