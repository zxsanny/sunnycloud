//! AC-1: the job migration applies, reverts and re-applies as the migrator role after the accounts, blob and
//! tree migrations, and creates exactly the tables, the generated column and the indexes of data_model 9.

use sc_db::{Db, Migrator};
use sqlx::{Connection, PgConnection};

use crate::support::{Fixture, JobRow, account_with_revision, constraint, job, sqlstate};

const ACCOUNTS_VERSION: i64 = 20_261_105_090_000;
const BLOB_VERSION: i64 = 20_261_106_120_000;
const TREE_VERSION: i64 = 20_261_106_130_000;
const JOB_VERSION: i64 = 20_261_106_160_000;
const TABLES: [&str; 3] = ["jobs", "derived", "worker_status"];
const CHECK_VIOLATION: &str = "23514";

type Column = (String, String, String);

fn column(name: &str, data_type: &str, nullable: &str) -> Column {
    (name.to_owned(), data_type.to_owned(), nullable.to_owned())
}

/// Reverts the job migration and everything applied after it.
async fn revert_job(migrator: &Db) {
    let status = Migrator::status(migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= JOB_VERSION)
        .count();
    Migrator::revert(migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the job migration");
}

async fn columns(conn: &mut PgConnection, table: &str) -> Vec<Column> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text \
         FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1 \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(conn)
    .await
    .expect("list columns")
}

async fn index_names(conn: &mut PgConnection, table: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT indexname::text FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename = $1 ORDER BY indexname",
    )
    .bind(table)
    .fetch_all(conn)
    .await
    .expect("list indexes")
}

async fn index_def(conn: &mut PgConnection, name: &str) -> String {
    sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND indexname = $1",
    )
    .bind(name)
    .fetch_one(conn)
    .await
    .expect("read index definition")
}

async fn existing_tables(conn: &mut PgConnection) -> Vec<&'static str> {
    let mut found = Vec::new();
    for table in TABLES {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("public.{table}"))
            .fetch_one(&mut *conn)
            .await
            .expect("look up table");
        if exists {
            found.push(table);
        }
    }
    found
}

async fn lane_generation(conn: &mut PgConnection) -> Option<String> {
    sqlx::query_scalar(
        "SELECT is_generated::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'jobs' AND column_name = 'lane'",
    )
    .fetch_optional(conn)
    .await
    .expect("read lane generation")
}

fn expected_indexes() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "jobs",
            vec![
                "jobs_claim_idx",
                "jobs_finished_idx",
                "jobs_key",
                "jobs_pkey",
                "jobs_queued_idx",
                "jobs_revision_id_idx",
            ],
        ),
        ("derived", vec!["derived_blob_id_key", "derived_pkey"]),
        ("worker_status", vec!["worker_status_pkey"]),
    ]
}

async fn all_indexes(conn: &mut PgConnection) -> Vec<(&'static str, Vec<String>)> {
    let mut found = Vec::new();
    for table in TABLES {
        found.push((table, index_names(conn, table).await));
    }
    found
}

#[tokio::test]
async fn up_down_up_creates_and_removes_the_tables_the_generated_column_and_the_indexes() {
    // Arrange
    let fixture = Fixture::new().await;
    let migrator = fixture.db.migrator().await;
    let mut conn = fixture.owner_conn().await;
    let expected: Vec<(&str, Vec<String>)> = expected_indexes()
        .into_iter()
        .map(|(table, names)| (table, names.into_iter().map(str::to_owned).collect()))
        .collect();

    // Act
    let tables_after_first_up = existing_tables(&mut conn).await;
    let lane_after_first_up = lane_generation(&mut conn).await;
    let indexes_after_first_up = all_indexes(&mut conn).await;
    revert_job(&migrator).await;
    let tables_after_down = existing_tables(&mut conn).await;
    let lane_after_down = lane_generation(&mut conn).await;
    let indexes_after_down = all_indexes(&mut conn).await;
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the job migration");
    let tables_after_second_up = existing_tables(&mut conn).await;
    let lane_after_second_up = lane_generation(&mut conn).await;
    let indexes_after_second_up = all_indexes(&mut conn).await;

    // Assert
    assert_eq!(tables_after_first_up, TABLES);
    assert_eq!(tables_after_second_up, TABLES);
    assert!(tables_after_down.is_empty(), "{tables_after_down:?}");
    assert_eq!(lane_after_first_up.as_deref(), Some("ALWAYS"));
    assert_eq!(lane_after_second_up.as_deref(), Some("ALWAYS"));
    assert_eq!(lane_after_down, None);
    assert_eq!(indexes_after_first_up, expected);
    assert_eq!(indexes_after_second_up, expected);
    assert!(indexes_after_down.iter().all(|(_, names)| names.is_empty()));
    conn.close().await.expect("close owner connection");
    drop(migrator);
    fixture.finish().await;
}

#[tokio::test]
async fn the_job_migration_sorts_after_the_accounts_blob_and_tree_migrations() {
    // Arrange
    let fixture = Fixture::new().await;
    let migrator = fixture.db.migrator().await;

    // Act
    let applied = Migrator::status(&migrator)
        .await
        .expect("migration status")
        .applied;

    // Assert
    let position = |version: i64| applied.iter().position(|v| *v == version);
    let job = position(JOB_VERSION).expect("job migration applied");
    for earlier in [ACCOUNTS_VERSION, BLOB_VERSION, TREE_VERSION] {
        assert!(
            position(earlier).expect("earlier migration applied") < job,
            "{applied:?}"
        );
    }
    drop(migrator);
    fixture.finish().await;
}

#[tokio::test]
async fn the_tables_have_the_columns_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let jobs = vec![
        column("id", "uuid", "NO"),
        column("account_id", "uuid", "NO"),
        column("revision_id", "uuid", "NO"),
        column("kind", "text", "NO"),
        column("lane", "text", "NO"),
        column("content_hash", "bytea", "NO"),
        column("params_version", "smallint", "NO"),
        column("priority", "smallint", "NO"),
        column("status", "text", "NO"),
        column("attempts", "smallint", "NO"),
        column("available_at", "timestamp with time zone", "NO"),
        column("leased_by", "text", "YES"),
        column("outcome", "text", "YES"),
        column("last_error", "text", "YES"),
        column("created_at", "timestamp with time zone", "NO"),
        column("finished_at", "timestamp with time zone", "YES"),
    ];
    let derived = vec![
        column("account_id", "uuid", "NO"),
        column("content_hash", "bytea", "NO"),
        column("kind", "text", "NO"),
        column("params_version", "smallint", "NO"),
        column("blob_id", "uuid", "NO"),
        column("media_type", "text", "NO"),
        column("size", "bigint", "NO"),
        column("width", "integer", "YES"),
        column("height", "integer", "YES"),
        column("duration_ms", "integer", "YES"),
        column("created_at", "timestamp with time zone", "NO"),
    ];
    let worker_status = vec![
        column("worker_id", "text", "NO"),
        column("seen_at", "timestamp with time zone", "NO"),
        column("image_age_days", "integer", "NO"),
        column("decoder_ok", "boolean", "NO"),
    ];

    // Act
    let found = (
        columns(&mut conn, "jobs").await,
        columns(&mut conn, "derived").await,
        columns(&mut conn, "worker_status").await,
    );

    // Assert
    assert_eq!(found, (jobs, derived, worker_status));
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn the_claim_finished_and_queued_indexes_are_partial_and_ordered_as_specified() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let claim = index_def(&mut conn, "jobs_claim_idx").await;
    let finished = index_def(&mut conn, "jobs_finished_idx").await;
    let queued = index_def(&mut conn, "jobs_queued_idx").await;
    let key = index_def(&mut conn, "jobs_key").await;
    let derived_pkey = index_def(&mut conn, "derived_pkey").await;

    // Assert
    assert_eq!(
        claim,
        "CREATE INDEX jobs_claim_idx ON public.jobs USING btree (lane, priority, available_at) \
         WHERE (status = ANY (ARRAY['queued'::text, 'running'::text]))"
    );
    assert_eq!(
        finished,
        "CREATE INDEX jobs_finished_idx ON public.jobs USING btree (finished_at) \
         WHERE (status = ANY (ARRAY['succeeded'::text, 'dead'::text]))"
    );
    assert_eq!(
        queued,
        "CREATE INDEX jobs_queued_idx ON public.jobs USING btree (account_id) WHERE (status = 'queued'::text)"
    );
    assert_eq!(
        key,
        "CREATE UNIQUE INDEX jobs_key ON public.jobs USING btree (account_id, kind, content_hash, params_version)"
    );
    assert_eq!(
        derived_pkey,
        "CREATE UNIQUE INDEX derived_pkey ON public.derived USING btree (account_id, content_hash, kind, params_version)"
    );
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn jobs_is_tuned_for_churn() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let options: Vec<String> = sqlx::query_scalar(
        "SELECT unnest(reloptions)::text FROM pg_class WHERE oid = 'public.jobs'::regclass ORDER BY 1",
    )
    .fetch_all(&mut conn)
    .await
    .expect("read storage options");

    // Assert
    assert_eq!(
        options,
        [
            "autovacuum_analyze_scale_factor=0.02",
            "autovacuum_vacuum_scale_factor=0.02",
            "fillfactor=70"
        ]
    );
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn jobs_refuses_a_value_outside_its_vocabulary() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = uuid::Uuid::new_v4();
    let revision = account_with_revision(&mut conn, account, &[1; 32]).await;
    let long_error = "x".repeat(501);
    let probes = [
        ("kind", "kind = 'thumbnail'"),
        ("priority", "priority = 2"),
        ("status", "status = 'paused'"),
        ("outcome", "outcome = 'lost'"),
        ("attempts", "attempts = -1"),
        ("content_hash", "content_hash = '\\x01'"),
    ];
    let job_id = job(&mut conn, &JobRow::new(account, revision, &[1; 32]))
        .await
        .expect("insert job");

    // Act
    let mut refused = Vec::new();
    for (name, assignment) in probes {
        let update = format!("UPDATE jobs SET {assignment} WHERE id = $1");
        let result = sqlx::query(sqlx::AssertSqlSafe(update))
            .bind(job_id)
            .execute(&mut conn)
            .await;
        refused.push((name, sqlstate(&result.expect_err("refused"))));
    }
    let too_long = sqlx::query("UPDATE jobs SET last_error = $2 WHERE id = $1")
        .bind(job_id)
        .bind(&long_error)
        .execute(&mut conn)
        .await;
    let at_limit = sqlx::query("UPDATE jobs SET last_error = $2 WHERE id = $1")
        .bind(job_id)
        .bind("x".repeat(500))
        .execute(&mut conn)
        .await;

    // Assert
    for (name, state) in refused {
        assert_eq!(state, CHECK_VIOLATION, "{name}");
    }
    assert_eq!(
        constraint(&too_long.expect_err("501 characters")),
        "jobs_last_error_check"
    );
    at_limit.expect("500 characters");
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}
