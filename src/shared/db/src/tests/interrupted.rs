//! AC-5: a migration whose backend is killed mid-way leaves the earlier migrations applied and itself
//! rolled back; the next run applies it. The kill is `pg_terminate_backend`, which ends the session exactly as
//! a killed migrating process does. Also verifies, against sqlx 0.9, what an interrupted `-- no-transaction`
//! migration leaves behind (task risk 1).

use std::time::{Duration, Instant};

use sqlx::PgConnection;

use super::accounts_base::EMBEDDED;
use super::seeded::{self, SLOW_GATE, SLOW_MARKER};
use super::support::TestDb;
use crate::migrate::MigrationSet;
use crate::{Db, MigrationError, SchemaState};

const SLOW: i64 = 20_991_231_000_002;

async fn table_exists(conn: &mut PgConnection, table: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1::text) IS NOT NULL")
        .bind(table)
        .fetch_one(conn)
        .await
        .expect("to_regclass")
}

/// Runs `set` on a task, waits until its slow statement is executing, terminates that backend, and returns
/// what the task returned.
async fn run_and_kill(
    db: &TestDb,
    migrator: &Db,
    set: MigrationSet,
) -> Result<crate::MigrationReport, MigrationError> {
    let mut admin = db.admin().await;
    sqlx::query("CREATE TABLE sc_slow_gate (id int)")
        .execute(&mut admin)
        .await
        .expect("close the gate");
    let target = migrator.clone();
    let task = tokio::spawn(async move { set.run_pending(&target).await });
    let deadline = Instant::now() + Duration::from_secs(30);
    let pid: i32 = loop {
        let found: Option<i32> = sqlx::query_scalar(
            "SELECT pid FROM pg_stat_activity WHERE datname = $1 AND state = 'active' \
             AND pid <> pg_backend_pid() AND query LIKE '%sc_slow_migration_marker%' LIMIT 1",
        )
        .bind(&db.name)
        .fetch_optional(&mut admin)
        .await
        .expect("look for the migrating backend");
        if let Some(pid) = found {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "the slow migration never started"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let terminated: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(pid)
        .fetch_one(&mut admin)
        .await
        .expect("terminate the migrating backend");
    assert!(terminated);
    let result = tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .expect("the killed run returns")
        .expect("the run task does not panic");
    sqlx::query("DROP TABLE sc_slow_gate")
        .execute(&mut admin)
        .await
        .expect("open the gate");
    result
}

#[tokio::test]
async fn a_killed_migration_rolls_back_and_the_next_run_applies_it() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let api = db.api().await;
    let slow_up =
        format!("{SLOW_MARKER}\nCREATE TABLE probe_slow (id int PRIMARY KEY);\n{SLOW_GATE}");
    let set =
        || MigrationSet::embedded().plus(seeded::pair(SLOW, &slow_up, "DROP TABLE probe_slow;"));

    let killed = run_and_kill(&db, &migrator, set()).await;

    assert!(
        matches!(
            killed,
            Err(MigrationError::MigrationFailed { version: SLOW, .. })
        ),
        "{killed:?}"
    );
    let mut admin = db.admin().await;
    assert!(
        table_exists(&mut admin, "accounts").await,
        "the base migration stays applied"
    );
    assert!(
        !table_exists(&mut admin, "probe_slow").await,
        "the killed migration leaves no change"
    );
    let status = set().status(&api).await.expect("status after the kill");
    assert_eq!(status.applied, EMBEDDED);
    assert_eq!(status.pending, [SLOW]);
    assert_eq!(status.dirty, None);
    assert_eq!(
        set().check_version(&api).await,
        Ok(SchemaState::Pending { n: 1 })
    );
    let rerun = set()
        .run_pending(&migrator)
        .await
        .expect("the next run applies it");
    assert_eq!(rerun.applied, [SLOW]);
    assert!(table_exists(&mut admin, "probe_slow").await);
    assert_eq!(set().check_version(&api).await, Ok(SchemaState::Current));
    db.drop_db().await;
}

#[tokio::test]
async fn a_killed_no_transaction_migration_is_pending_not_dirty_and_reruns_idempotently() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let api = db.api().await;
    let no_tx_up = format!(
        "-- no-transaction\n{SLOW_MARKER}\nBEGIN; CREATE TABLE IF NOT EXISTS probe_no_tx (id int PRIMARY KEY); COMMIT;\n\
         {SLOW_GATE}\nCREATE INDEX IF NOT EXISTS probe_no_tx_idx ON probe_no_tx (id);"
    );
    let set = || {
        MigrationSet::embedded().plus(seeded::pair(
            SLOW,
            &no_tx_up,
            "-- no-transaction\nDROP TABLE IF EXISTS probe_no_tx;",
        ))
    };

    let killed = run_and_kill(&db, &migrator, set()).await;

    assert!(
        matches!(
            killed,
            Err(MigrationError::MigrationFailed { version: SLOW, .. })
        ),
        "{killed:?}"
    );
    let mut admin = db.admin().await;
    assert!(
        table_exists(&mut admin, "probe_no_tx").await,
        "the part committed before the kill remains: why no-transaction files must be idempotent"
    );
    let status = set().status(&api).await.expect("status after the kill");
    assert_eq!(
        status.pending,
        [SLOW],
        "sqlx 0.9 records no row for the interrupted file"
    );
    assert_eq!(
        status.dirty, None,
        "sqlx 0.9 never writes success = false on PostgreSQL"
    );
    assert_eq!(
        set().check_version(&api).await,
        Ok(SchemaState::Pending { n: 1 })
    );
    let rerun = set()
        .run_pending(&migrator)
        .await
        .expect("the idempotent file reruns");
    assert_eq!(rerun.applied, [SLOW]);
    db.drop_db().await;
}
