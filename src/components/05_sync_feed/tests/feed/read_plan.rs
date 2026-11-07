//! AC-8 (PT-05-2): the page statement reaches `nodes` through `nodes_feed_idx` and never scans the table. The
//! plan check runs everywhere on a 30,000-node account; the latency run needs the 1,000,000-node account of
//! REF-SRV and is ignored elsewhere (`NOT-RUN(HW-REF)`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::Cursor;
use sc_db::{Db, DbConfig, DbRole, Scope};
use sqlx::{AssertSqlSafe, Connection};
use sunnycloud_sync_feed::{PgFeedReader, READ_PAGE_SQL};
use uuid::Uuid;

use crate::read_support::{EPOCH, feed_config};
use crate::stamp::Stamped;
use crate::stamp::account_with_nodes;

const BATCH: i64 = 500;

/// Gives the account `count` folders stamped in batches of 500 (seq 1, 2, ...), moves the counter to match and
/// refreshes the planner statistics the way autovacuum would have.
async fn stamp_in_batches(stamped: &Stamped, count: i64) -> i64 {
    let mut conn = stamped.fixture.owner_conn().await;
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at, seq) \
         SELECT gen_random_uuid(), $1, $2, 'folder', 'n' || g, 'n' || g, 1, 'epoch'::timestamptz, 'epoch'::timestamptz, (g - 1) / $4 + 1 \
         FROM generate_series(1::bigint, $3) g",
    )
    .bind(stamped.account)
    .bind(stamped.root)
    .bind(count)
    .bind(BATCH)
    .execute(&mut conn)
    .await
    .expect("bulk nodes");
    let last = (count - 1) / BATCH + 1;
    sqlx::query("UPDATE account_feeds SET seq = $2 WHERE account_id = $1")
        .bind(stamped.account)
        .bind(last)
        .execute(&mut conn)
        .await
        .expect("move the counter");
    sqlx::query("ANALYZE nodes")
        .execute(&mut conn)
        .await
        .expect("analyze");
    conn.close().await.expect("close owner connection");
    last
}

async fn plan(db: &Db, account: Uuid, at: Cursor) -> String {
    let explain = format!("EXPLAIN (COSTS OFF) {READ_PAGE_SQL}");
    let lines: Vec<String> = db
        .run(Scope::Account(account), async |tx| {
            sqlx::query_scalar(AssertSqlSafe(explain))
                .bind(account)
                .bind(at.epoch)
                .bind(at.seq)
                .bind(BATCH)
                .fetch_all(tx.conn())
                .await
        })
        .await
        .expect("explain");
    lines.join("\n")
}

fn assert_index_plan(plan: &str) {
    assert!(plan.contains("nodes_feed_idx"), "{plan}");
    assert!(!plan.contains("Seq Scan on nodes"), "{plan}");
}

#[tokio::test]
async fn a_catch_up_page_reads_nodes_through_the_feed_index_and_not_by_scanning_them() {
    // Arrange
    let (stamped, _) = account_with_nodes(1).await;
    let last = stamp_in_batches(&stamped, 30_000).await;

    // Act
    let catch_up = plan(
        &stamped.fixture.api,
        stamped.account,
        Cursor {
            epoch: EPOCH,
            seq: last - 3,
        },
    )
    .await;
    let head_read = plan(
        &stamped.fixture.api,
        stamped.account,
        Cursor {
            epoch: EPOCH,
            seq: last,
        },
    )
    .await;

    // Assert
    assert_index_plan(&catch_up);
    assert_index_plan(&head_read);
    stamped.fixture.finish().await;
}

const READERS: usize = 20;
const READS: usize = 5_000;
const READER_CONNECTIONS: u32 = 8;

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

/// PT-05-2 on REF-SRV only (`cargo test -- --ignored`); `PT_05_2_NODES` shrinks the account for a smoke check,
/// the thresholds apply unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-05-2 timing is measured on REF-SRV only"]
async fn pt_05_2_twenty_readers_on_a_million_nodes_meet_the_page_latency_target() {
    // Arrange
    let nodes: i64 = std::env::var("PT_05_2_NODES")
        .ok()
        .map_or(1_000_000, |text| {
            text.parse().expect("PT_05_2_NODES is a number")
        });
    let (stamped, _) = account_with_nodes(1).await;
    let last = stamp_in_batches(&stamped, nodes).await;
    let config = DbConfig {
        db_pool_max: READER_CONNECTIONS,
        ..stamped.fixture.db.config()
    };
    let readers = Db::connect(&config, DbRole::Api)
        .await
        .expect("reader pool");
    let reader = Arc::new(PgFeedReader::new(readers.clone(), &feed_config(500, None)));
    let account = stamped.account;

    // Act
    let tasks: Vec<_> = (0..READERS)
        .map(|index| {
            let reader = reader.clone();
            tokio::spawn(async move {
                let mut latencies = Vec::new();
                for read in 0..READS / READERS {
                    let seq =
                        (i64::try_from(index * 7919 + read * 104_729).expect("fits") % last).max(0);
                    let begun = Instant::now();
                    let page = reader
                        .read_page(account, Cursor { epoch: EPOCH, seq }, None)
                        .await
                        .expect("page");
                    latencies.push(begun.elapsed());
                    assert!(!page.items.is_empty() || seq == last);
                }
                latencies
            })
        })
        .collect();
    let mut latencies = Vec::new();
    for task in tasks {
        latencies.extend(task.await.expect("reader task"));
    }
    latencies.sort_unstable();
    let p95 = percentile(&latencies, 95);
    let plan_text = plan(
        &readers,
        account,
        Cursor {
            epoch: EPOCH,
            seq: last - 1,
        },
    )
    .await;

    // Assert
    println!("PT-05-2: {} reads, p95 {p95:?}", latencies.len());
    assert!(p95 <= Duration::from_millis(20), "p95 {p95:?}");
    assert_index_plan(&plan_text);
    drop(reader);
    drop(readers);
    stamped.fixture.finish().await;
}
