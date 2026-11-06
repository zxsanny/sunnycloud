//! AC-1: the identity migration applies, reverts and re-applies on a real PostgreSQL 18 as the migrator role.

mod support;

use sc_db::Migrator;
use sqlx::{Connection, PgConnection};
use support::Harness;

const IDENTITY_VERSION: i64 = 20_261_105_110_000;
const IDENTITY_TABLES: [&str; 7] = [
    "device_grants",
    "device_tokens",
    "identities",
    "invites",
    "login_codes",
    "oauth_states",
    "sessions",
];

#[derive(Debug, PartialEq, Eq)]
struct Shape {
    identity_tables: i64,
    accounts_columns: Vec<String>,
    state_index: i64,
}

async fn shape(conn: &mut PgConnection) -> Shape {
    let identity_tables = sqlx::query_scalar(
        "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tablename = ANY($1)",
    )
    .bind(IDENTITY_TABLES.map(String::from).to_vec())
    .fetch_one(&mut *conn)
    .await
    .expect("count identity tables");
    let accounts_columns = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'accounts' ORDER BY column_name",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("list accounts columns");
    let state_index = sqlx::query_scalar(
        "SELECT count(*) FROM pg_indexes WHERE tablename = 'accounts' AND indexname = 'accounts_state_idx'",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("count accounts_state_idx");
    Shape {
        identity_tables,
        accounts_columns,
        state_index,
    }
}

/// Reverts the identity migration and everything applied after it.
async fn revert_identity(db: &sc_db::Db) {
    let status = Migrator::status(db).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= IDENTITY_VERSION)
        .count();
    Migrator::revert(db, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the identity migration");
}

fn applied_shape() -> Shape {
    Shape {
        identity_tables: 7,
        accounts_columns: [
            "created_at",
            "deletion_requested_at",
            "display_name",
            "id",
            "role",
            "state",
        ]
        .map(String::from)
        .to_vec(),
        state_index: 1,
    }
}

#[tokio::test]
async fn up_down_up_succeeds_and_leaves_the_identity_schema() {
    // Arrange
    let harness = Harness::new().await;
    let migrator = harness.db.migrator().await;

    // Act
    revert_identity(&migrator).await;
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the identity migration");
    let mut conn = harness.fixture_conn().await;
    let after = shape(&mut conn).await;

    // Assert
    assert_eq!(after, applied_shape());
    conn.close().await.expect("close fixture connection");
    drop(migrator);
    harness.finish().await;
}

#[tokio::test]
async fn down_removes_identity_tables_columns_and_index() {
    // Arrange
    let harness = Harness::new().await;
    let migrator = harness.db.migrator().await;

    // Act
    revert_identity(&migrator).await;
    let mut conn = harness.fixture_conn().await;
    let after = shape(&mut conn).await;

    // Assert
    assert_eq!(
        after,
        Shape {
            identity_tables: 0,
            accounts_columns: ["created_at", "id", "state"].map(String::from).to_vec(),
            state_index: 0,
        }
    );
    conn.close().await.expect("close fixture connection");
    drop(migrator);
    harness.finish().await;
}
