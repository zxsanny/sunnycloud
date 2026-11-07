//! AC-69 (component part) and AC-5 (PT-05-1): real concurrent transactions on one account, each writing a node
//! and stamping as its last statement, some rolling back. One pooled connection per writer.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{FeedStamp, Seq, TouchedNodes};
use sc_db::{Db, RunError, Scope};
use sqlx::Connection;
use sunnycloud_sync_feed::PgFeedStamp;
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::stamp::Attempt;
use crate::support::{Fixture, account_with_feed, insert_folders, root_node};

const WRITERS: usize = 50;
const ROUNDS: usize = 100;
const ROLLBACK_EVERY: usize = 5;

#[derive(Clone)]
struct Writer {
    db: Db,
    account: Uuid,
    root: Uuid,
}

impl Writer {
    async fn transaction(&self, commit: bool) -> Result<Seq, RunError<Attempt>> {
        self.db
            .run(Scope::Account(self.account), async |tx| {
                let node = insert_folders(tx.conn(), self.account, self.root, 1).await?;
                let seq = PgFeedStamp
                    .stamp(tx, self.account, TouchedNodes(node))
                    .await?;
                if commit {
                    Ok(seq)
                } else {
                    Err(Attempt::Rollback)
                }
            })
            .await
    }
}

async fn one_account(pool_max: u32) -> (Fixture, Writer) {
    let fixture = Fixture::with_pool_max(pool_max).await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    account_with_feed(&mut conn, account, 1).await;
    let root = root_node(&mut conn, account).await;
    conn.close().await.expect("close owner connection");
    let writer = Writer {
        db: fixture.api.clone(),
        account,
        root,
    };
    (fixture, writer)
}

/// Fails the test on anything but a commit or the deliberate rollback: a deadlock, a serialization failure or any
/// other error reaching a caller.
fn committed(result: Result<Seq, RunError<Attempt>>) -> Option<Seq> {
    match result {
        Ok(seq) => Some(seq),
        Err(RunError::Closure(Attempt::Rollback)) => None,
        Err(RunError::Closure(Attempt::Failed(error))) => panic!("stamp failed: {error}"),
        Err(RunError::Closure(Attempt::Db(error))) | Err(RunError::Db(error)) => {
            panic!("database failure reached the caller: {error}")
        }
    }
}

async fn assert_gapless(fixture: &Fixture, writer: &Writer, seqs: &mut [Seq]) {
    seqs.sort_unstable();
    let count = i64::try_from(seqs.len()).expect("count fits");
    let expected: Vec<Seq> = (1..=count).collect();
    assert_eq!(seqs, expected.as_slice(), "committed stamps are 1..=N");
    let mut conn = fixture.owner_conn().await;
    let counter: i64 = sqlx::query_scalar("SELECT seq FROM account_feeds WHERE account_id = $1")
        .bind(writer.account)
        .fetch_one(&mut conn)
        .await
        .expect("read the counter");
    let node_seqs: Vec<i64> =
        sqlx::query_scalar("SELECT seq FROM nodes WHERE account_id = $1 AND seq > 0 ORDER BY seq")
            .bind(writer.account)
            .fetch_all(&mut conn)
            .await
            .expect("read node seqs");
    conn.close().await.expect("close owner connection");
    assert_eq!(counter, count);
    assert_eq!(
        node_seqs, expected,
        "each committed node carries its own seq"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_writers_over_a_hundred_rounds_with_rollbacks_stamp_each_integer_once() {
    // Arrange
    let (fixture, writer) = one_account(WRITERS as u32).await;
    let mut seqs = Vec::new();
    let mut rolled_back = 0;

    // Act
    for round in 0..ROUNDS {
        let barrier = Arc::new(Barrier::new(WRITERS));
        let tasks: Vec<_> = (0..WRITERS)
            .map(|index| {
                let (writer, barrier) = (writer.clone(), barrier.clone());
                let commit = !(round * WRITERS + index).is_multiple_of(ROLLBACK_EVERY);
                tokio::spawn(async move {
                    barrier.wait().await;
                    writer.transaction(commit).await
                })
            })
            .collect();
        for task in tasks {
            match committed(task.await.expect("writer task")) {
                Some(seq) => seqs.push(seq),
                None => rolled_back += 1,
            }
        }
    }

    // Assert
    assert_eq!(rolled_back, ROUNDS * WRITERS / ROLLBACK_EVERY);
    assert_eq!(seqs.len(), ROUNDS * WRITERS - rolled_back);
    assert_gapless(&fixture, &writer, &mut seqs).await;
    drop(writer);
    fixture.finish().await;
}

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

/// PT-05-1 runs on REF-SRV only (`cargo test -- --ignored`); `PT_05_1_SECONDS` shortens the run for a smoke
/// check, the thresholds apply unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-05-1 timing is measured on REF-SRV only"]
async fn pt_05_1_fifty_writers_closed_loop_meet_the_throughput_and_latency_targets() {
    // Arrange
    let seconds: u64 = std::env::var("PT_05_1_SECONDS").ok().map_or(120, |text| {
        text.parse().expect("PT_05_1_SECONDS is a number")
    });
    let (fixture, writer) = one_account(WRITERS as u32).await;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds);

    // Act
    let tasks: Vec<_> = (0..WRITERS)
        .map(|_| {
            let writer = writer.clone();
            tokio::spawn(async move {
                let mut done = Vec::new();
                while Instant::now() < deadline {
                    let begun = Instant::now();
                    let seq = committed(writer.transaction(true).await).expect("commit");
                    done.push((seq, begun.elapsed()));
                }
                done
            })
        })
        .collect();
    let mut done = Vec::new();
    for task in tasks {
        done.extend(task.await.expect("writer task"));
    }
    let elapsed = started.elapsed();

    // Assert
    let mut seqs: Vec<Seq> = done.iter().map(|(seq, _)| *seq).collect();
    let mut latencies: Vec<Duration> = done.iter().map(|(_, latency)| *latency).collect();
    latencies.sort_unstable();
    let rate = seqs.len() as f64 / elapsed.as_secs_f64();
    let (p95, p99) = (percentile(&latencies, 95), percentile(&latencies, 99));
    println!(
        "PT-05-1: {} stamps, {rate:.0}/s, p95 {p95:?}, p99 {p99:?}",
        seqs.len()
    );
    assert_gapless(&fixture, &writer, &mut seqs).await;
    assert!(rate >= 1000.0, "throughput {rate:.0}/s, target 1000/s");
    assert!(p95 <= Duration::from_millis(50), "p95 {p95:?}");
    assert!(p99 <= Duration::from_millis(100), "p99 {p99:?}");
    drop(writer);
    fixture.finish().await;
}
