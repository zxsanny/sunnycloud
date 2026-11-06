//! The harness itself, against the real server: what a clone holds, the roles on it and the connection count.

use sc_testdb::{API, MIGRATOR, START, TestDb, clock_at_start};

const AUDIT_OWNER: &str = "SELECT count(*) FROM pg_class c JOIN pg_roles r ON r.oid = c.relowner \
                           WHERE c.relname = 'audit_log' AND r.rolname = 'sunnycloud_migrator'";
const PUBLIC_OWNER: &str = "SELECT count(*) FROM pg_namespace n JOIN pg_roles r ON r.oid = n.nspowner \
                            WHERE n.nspname = 'public' AND r.rolname = 'sunnycloud_migrator'";

#[tokio::test]
async fn clone_template_holds_the_shipped_tables_owned_by_the_migrator() {
    // Arrange
    let db = TestDb::clone_template().await;

    // Act
    let tables = db.admin_count(AUDIT_OWNER).await;
    let schema = db.admin_count(PUBLIC_OWNER).await;

    // Assert
    assert_eq!((tables, schema), (1, 1));
    db.drop_db().await;
}

#[tokio::test]
async fn clone_empty_has_the_schema_setup_and_no_tables() {
    // Arrange
    let db = TestDb::clone_empty().await;

    // Act
    let tables = db
        .admin_count("SELECT count(*) FROM pg_class WHERE relname = 'audit_log'")
        .await;
    let schema = db.admin_count(PUBLIC_OWNER).await;

    // Assert
    assert_eq!((tables, schema), (0, 1));
    db.drop_db().await;
}

#[tokio::test]
async fn conn_as_connects_as_the_named_role() {
    // Arrange
    let db = TestDb::clone_template().await;

    // Act
    let mut api = db.conn_as(API).await;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&mut api)
        .await
        .expect("current_user");
    let migrator_conn = db.conn_as(MIGRATOR).await;

    // Assert
    assert_eq!(user, API);
    drop((api, migrator_conn));
    db.drop_db().await;
}

#[tokio::test]
async fn finish_reports_zero_connections_once_the_pools_are_dropped() {
    // Arrange
    let db = TestDb::clone_template().await;
    let api = db.api().await;
    let open_while_held = db.open_connections().await;

    // Act
    drop(api);
    let remaining = db.finish().await;

    // Assert
    assert!(open_while_held >= 1, "the pool held no connection");
    assert_eq!(remaining, 0);
}

#[test]
fn clock_at_start_stands_at_start() {
    // Arrange
    let clock = clock_at_start();

    // Act
    let now = sc_clock::Clock::now_utc(clock.as_ref());

    // Assert
    assert_eq!(now, START);
}
