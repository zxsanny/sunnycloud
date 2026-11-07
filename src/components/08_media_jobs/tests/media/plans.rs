//! AC-4 (PT-08-1): with 100,000 queued jobs over two lanes, the claim candidate query walks `jobs_claim_idx`
//! in `(priority, available_at)` order without a sort, and the per-account queued count uses `jobs_queued_idx`.
//! The plan is read as the schema owner here and as the API role in system scope in `system_scope_repo.rs`.
//!
//! The rows are bulk-inserted by the superuser with foreign-key triggers off: the plan depends on the data of the
//! indexed columns, and 100,000 real revisions would only slow the seeding. The foreign keys are proven in `keys.rs`.

use sqlx::{AssertSqlSafe, Connection, PgConnection};
use uuid::Uuid;

use crate::support::Fixture;

pub const JOBS: i64 = 100_000;
pub const ACCOUNTS: i64 = 10;
pub const THRESHOLD: i64 = 200;
const NOW: &str = "2026-11-06 12:00:00+00";

pub struct Queue {
    pub fixture: Fixture,
    pub account: Uuid,
}

pub fn claim_sql(lane: &str) -> String {
    format!(
        "SELECT id FROM jobs WHERE lane = '{lane}' AND status IN ('queued', 'running') \
         AND available_at <= '{NOW}'::timestamptz AND attempts < 3 \
         ORDER BY priority, available_at FOR UPDATE SKIP LOCKED LIMIT 1"
    )
}

pub fn count_sql(account: Uuid) -> String {
    format!(
        "SELECT count(*) FROM (SELECT 1 FROM jobs WHERE account_id = '{account}' AND status = 'queued' \
         LIMIT {}) AS capped",
        THRESHOLD + 1
    )
}

/// 100,000 queued jobs over ten accounts, half of them `stills` (lane `image`) and half `poster` or `transcode`
/// (lane `video`), one job in five bulk, due times spread over a day; statistics and visibility map are current.
pub async fn seed_queue() -> Queue {
    let fixture = Fixture::new().await;
    let mut admin = fixture.db.admin().await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         SELECT md5('account' || n)::uuid, 'active', 'epoch'::timestamptz, 'Fixture', 'user' \
         FROM generate_series(0, $1 - 1) AS n",
    )
    .bind(ACCOUNTS)
    .execute(&mut admin)
    .await
    .expect("insert accounts");
    sqlx::raw_sql("SET session_replication_role = replica")
        .execute(&mut admin)
        .await
        .expect("disable foreign-key triggers");
    sqlx::query(
        "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
                           available_at, created_at) \
         SELECT md5('job' || n)::uuid, md5('account' || (n % $1))::uuid, md5('revision' || n)::uuid, \
                (ARRAY['stills', 'stills', 'poster', 'transcode'])[1 + n % 4], \
                sha256(convert_to(n::text, 'UTF8')), 1, CASE WHEN n % 5 = 0 THEN 1 ELSE 0 END, 'queued', \
                '2026-11-05 12:00:00+00'::timestamptz + (n % 86400) * interval '1 second', \
                'epoch'::timestamptz \
         FROM generate_series(0, $2 - 1) AS n",
    )
    .bind(ACCOUNTS)
    .bind(JOBS)
    .execute(&mut admin)
    .await
    .expect("insert jobs");
    sqlx::raw_sql("VACUUM (ANALYZE) jobs")
        .execute(&mut admin)
        .await
        .expect("vacuum and analyze");
    let account: Uuid = sqlx::query_scalar("SELECT md5('account3')::uuid")
        .fetch_one(&mut admin)
        .await
        .expect("pick an account");
    admin.close().await.expect("close admin connection");
    Queue { fixture, account }
}

/// The lines of `EXPLAIN` for `sql`.
pub async fn explain(conn: &mut PgConnection, sql: &str) -> String {
    let lines: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN {sql}")))
        .fetch_all(conn)
        .await
        .unwrap_or_else(|error| panic!("explain failed: {error}\n{sql}"));
    lines.join("\n")
}

pub fn assert_claim_plan(plan: &str, lane: &str) {
    assert!(plan.contains("using jobs_claim_idx"), "{lane}: {plan}");
    assert!(!plan.contains("Seq Scan"), "{lane}: {plan}");
    assert!(!plan.contains("Sort"), "{lane}: {plan}");
    assert!(!plan.contains("Bitmap"), "{lane}: {plan}");
}

pub fn assert_count_plan(plan: &str) {
    assert!(plan.contains("jobs_queued_idx"), "{plan}");
    assert!(!plan.contains("Seq Scan"), "{plan}");
}

#[tokio::test]
async fn seeding_gives_100000_queued_jobs_over_two_lanes() {
    // Arrange
    let queue = seed_queue().await;
    let mut conn = queue.fixture.owner_conn().await;

    // Act
    let lanes: Vec<(String, i64)> = sqlx::query_as(
        "SELECT lane, count(*) FROM jobs WHERE status = 'queued' GROUP BY lane ORDER BY lane",
    )
    .fetch_all(&mut conn)
    .await
    .expect("count by lane");

    // Assert
    assert_eq!(
        lanes,
        [("image".to_owned(), 50_000), ("video".to_owned(), 50_000)]
    );
    conn.close().await.expect("close owner connection");
    queue.fixture.finish().await;
}

#[tokio::test]
async fn the_claim_candidate_query_walks_the_claim_index_in_order_on_both_lanes() {
    // Arrange
    let queue = seed_queue().await;
    let mut conn = queue.fixture.owner_conn().await;

    // Act
    let image = explain(&mut conn, &claim_sql("image")).await;
    let video = explain(&mut conn, &claim_sql("video")).await;

    // Assert
    assert_claim_plan(&image, "image");
    assert_claim_plan(&video, "video");
    conn.close().await.expect("close owner connection");
    queue.fixture.finish().await;
}

#[tokio::test]
async fn the_per_account_queued_count_uses_the_queued_index() {
    // Arrange
    let queue = seed_queue().await;
    let mut conn = queue.fixture.owner_conn().await;

    // Act
    let plan = explain(&mut conn, &count_sql(queue.account)).await;
    let counted: i64 = sqlx::query_scalar(AssertSqlSafe(count_sql(queue.account)))
        .fetch_one(&mut conn)
        .await
        .expect("run the count");

    // Assert
    assert_count_plan(&plan);
    assert_eq!(counted, THRESHOLD + 1);
    conn.close().await.expect("close owner connection");
    queue.fixture.finish().await;
}
