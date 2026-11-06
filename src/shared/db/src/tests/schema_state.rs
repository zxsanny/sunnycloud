//! AC-4 (schema states), AC-6 (applied files are immutable), AC-7 (irreversible down) and AC-8 (the API
//! role cannot migrate) on a real PostgreSQL 18. A "binary with one more file" is the embedded set plus a
//! seeded pair; see `seeded`.

use sqlx::PgConnection;

use super::accounts_base::{BASE_VERSION, embedded};
use super::seeded::{self, IRREVERSIBLE_DOWN};
use super::support::TestDb;
use crate::migrate::MigrationSet;
use crate::{DbError, MigrationError, Migrator, RunError, SchemaState};

const EXTRA: i64 = 20_991_231_000_001;

async fn table_exists(conn: &mut PgConnection, table: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1::text) IS NOT NULL")
        .bind(table)
        .fetch_one(conn)
        .await
        .expect("to_regclass")
}

/// Every embedded version, then the extra one the test adds.
fn with_extra() -> Vec<i64> {
    let mut versions = embedded();
    versions.push(EXTRA);
    versions
}

#[tokio::test]
async fn check_version_tells_current_pending_and_unknown_newer_apart() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let api = db.api().await;
    let one_more = || MigrationSet::embedded().plus(seeded::table_pair(EXTRA, "probe_extra"));
    Migrator::run_pending(&migrator).await.expect("migrate");

    let current = Migrator::check_version(&api).await;
    let pending = one_more().check_version(&api).await;
    one_more()
        .run_pending(&migrator)
        .await
        .expect("migrate the extra file");
    let unknown = Migrator::check_version(&api).await;

    assert_eq!(current, Ok(SchemaState::Current));
    assert_eq!(pending, Ok(SchemaState::Pending { n: 1 }));
    assert_eq!(unknown, Ok(SchemaState::UnknownNewer));
    let status = Migrator::status(&api)
        .await
        .expect("status as the API role");
    assert_eq!(status.applied, with_extra());
    assert_eq!(status.unknown_newer, [EXTRA]);
    assert!(status.pending.is_empty() && status.dirty.is_none());
    let refused = Migrator::run_pending(&migrator).await;
    assert_eq!(
        refused,
        Err(MigrationError::UnknownNewer { version: EXTRA })
    );
    db.drop_db().await;
}

#[tokio::test]
async fn a_changed_applied_file_is_a_checksum_mismatch_and_nothing_is_applied() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let api = db.api().await;
    Migrator::run_pending(&migrator).await.expect("migrate");
    let edited = || {
        MigrationSet::embedded()
            .with_up_sql(
                BASE_VERSION,
                "SELECT 1; -- the file was edited after it was applied",
            )
            .plus(seeded::table_pair(EXTRA, "probe_never"))
    };

    let run = edited().run_pending(&migrator).await;
    let check = edited().check_version(&api).await;

    assert_eq!(
        run,
        Err(MigrationError::ChecksumMismatch {
            version: BASE_VERSION
        })
    );
    assert_eq!(
        check,
        Err(MigrationError::ChecksumMismatch {
            version: BASE_VERSION
        })
    );
    let mut admin = db.admin().await;
    assert!(!table_exists(&mut admin, "probe_never").await);
    let status = Migrator::status(&api).await.expect("status");
    assert_eq!(status.applied, embedded());
    db.drop_db().await;
}

#[tokio::test]
async fn revert_reports_an_irreversible_down_and_leaves_the_schema() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let set = || {
        MigrationSet::embedded().plus(seeded::pair(
            EXTRA,
            "CREATE TABLE probe_irreversible (id int PRIMARY KEY);",
            IRREVERSIBLE_DOWN,
        ))
    };
    set().run_pending(&migrator).await.expect("migrate");

    let reverted = set().revert(&migrator, 1).await;

    assert_eq!(
        reverted,
        Err(MigrationError::IrreversibleMigration { version: EXTRA })
    );
    let mut admin = db.admin().await;
    assert!(table_exists(&mut admin, "probe_irreversible").await);
    assert!(table_exists(&mut admin, "accounts").await);
    let status = set().status(&migrator).await.expect("status");
    assert_eq!(status.applied, with_extra());
    db.drop_db().await;
}

#[tokio::test]
async fn a_failing_migration_is_named_and_earlier_ones_stay() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let set = || {
        MigrationSet::embedded().plus(seeded::pair(
            EXTRA,
            "CREATE TABLE probe_half (id int); SELECT 1 / 0;",
            "DROP TABLE probe_half;",
        ))
    };

    let run = set().run_pending(&migrator).await;

    assert!(
        matches!(
            run,
            Err(MigrationError::MigrationFailed { version: EXTRA, .. })
        ),
        "{run:?}"
    );
    let mut admin = db.admin().await;
    assert!(table_exists(&mut admin, "accounts").await);
    assert!(!table_exists(&mut admin, "probe_half").await);
    let status = set().status(&migrator).await.expect("status");
    assert_eq!(status.applied, embedded());
    assert_eq!(status.pending, [EXTRA]);
    db.drop_db().await;
}

#[tokio::test]
async fn the_api_role_cannot_migrate_but_can_read_the_version() {
    let db = TestDb::empty().await;
    let api = db.api().await;
    let migrator = db.migrator().await;

    let run = Migrator::run_pending(&api).await;
    let revert = Migrator::revert(&api, 1).await;
    let before = (
        Migrator::status(&api).await.map(|status| status.pending),
        Migrator::check_version(&api).await,
    );

    assert_eq!(run, Err(MigrationError::WrongRole));
    assert_eq!(revert, Err(MigrationError::WrongRole));
    assert_eq!(
        before,
        (
            Ok(embedded()),
            Ok(SchemaState::Pending {
                n: embedded().len()
            })
        )
    );
    let mut admin = db.admin().await;
    assert!(!table_exists(&mut admin, "accounts").await);
    assert!(!table_exists(&mut admin, "_sqlx_migrations").await);
    Migrator::run_pending(&migrator).await.expect("migrate");
    assert_eq!(
        Migrator::check_version(&api).await,
        Ok(SchemaState::Current)
    );
    db.drop_db().await;
}

#[tokio::test]
async fn the_database_itself_refuses_ddl_to_the_api_role() {
    let db = TestDb::empty().await;
    let api = db.api().await;

    let ddl = api
        .run_system("test_api_ddl", async |tx| {
            sqlx::query("CREATE TABLE api_made_this (id int)")
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await;

    assert!(
        matches!(ddl, Err(RunError::Closure(DbError::Other(ref text))) if text.contains("42501")),
        "{ddl:?}"
    );
    db.drop_db().await;
}
