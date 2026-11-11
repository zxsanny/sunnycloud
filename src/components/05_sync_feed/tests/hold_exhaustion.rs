//! AC-3 (ST-05-3): hold exhaustion. One principal opens 1,000 reads with `wait = 30 s`, then 100 principals open
//! 100 each (10,000). At most 4 reads per principal park, every other read is refused with `TooManyHolds` and the
//! `PROPOSED_HOLD_RETRY_AFTER`, other principals are unaffected, parked reads hold no connection, tasks stay at the
//! parked reads plus a constant, and the resident set grows by at most 32 MB. This binary holds this one test, so
//! the resident-set reading is not disturbed by other suites' allocations in the process.
//!
//! Reads open in waves of 500 so the 5 s pool acquire timeout is not what is measured. A refused read has already
//! made its one short first-page query (a read that finds items must not be refused), so connections are bounded
//! by the pool: it is configured with 2 connections and the baseline is taken with both open.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    ChangeFeed, ChangeFeedError, ChangePage, Cursor, Principal, PrincipalKind,
    PrincipalRevalidator, RevalidatorError, Role, async_trait,
};
use sc_clock::{CancellationToken, ManualClock};
use sc_db::{Db, DbConfig, DbRole};
use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::Connection;
use sunnycloud_sync_feed::{
    FeedConfig, InProcessNotifier, PROPOSED_HOLD_RETRY_AFTER, PgChangeFeed, PgFeedReader,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

const EPOCH: i64 = 7;
const CAP: usize = 4;
const WAIT: Duration = Duration::from_secs(30);
const WAVE: usize = 500;
const HANG_GUARD: Duration = Duration::from_secs(120);
const RSS_BUDGET_BYTES: u64 = 32 * 1024 * 1024;
const CONNECTIONS_ABOVE_BASELINE: i64 = 2;
const TASKS_ABOVE_PARKED: usize = 32;

type Held = JoinHandle<Result<ChangePage, ChangeFeedError>>;

struct Valid;

#[async_trait]
impl PrincipalRevalidator for Valid {
    async fn revalidate(&self, _principal: &Principal) -> Result<bool, RevalidatorError> {
        Ok(true)
    }
}

fn resident_bytes() -> u64 {
    let statm = fs::read_to_string("/proc/self/statm").expect("read statm");
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .and_then(|field| field.parse().ok())
        .expect("resident pages");
    pages * 4096
}

async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let polled = async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    tokio::time::timeout(HANG_GUARD, polled)
        .await
        .unwrap_or_else(|_| panic!("never happened: {what}"));
}

fn principal(account: Uuid) -> Principal {
    Principal {
        account_id: account,
        kind: PrincipalKind::Session,
        session_or_device_id: Uuid::new_v4(),
        role: Role::User,
        label: "test".to_owned(),
    }
}

fn at_start() -> Cursor {
    Cursor {
        epoch: EPOCH,
        seq: 0,
    }
}

struct Env {
    db: TestDb,
    api: Db,
    feed: Arc<PgChangeFeed>,
    notifier: Arc<InProcessNotifier>,
    shutdown: CancellationToken,
}

impl Env {
    async fn new() -> Env {
        let db = TestDb::clone_template().await;
        let api = Db::connect(
            &DbConfig {
                db_pool_max: 2,
                ..db.config()
            },
            DbRole::Api,
        )
        .await
        .expect("connect as the API role");
        let config = FeedConfig {
            longpoll_wait_max: WAIT,
            longpoll_auth_recheck: Duration::from_secs(2),
            feed_safety_recheck: Duration::from_secs(10),
            feed_page_limit_default: 500,
            feed_page_limit_max: None,
            max_holds_per_principal: CAP as u64,
        };
        let notifier = InProcessNotifier::new();
        let shutdown = CancellationToken::new();
        let metrics = sc_observability::Metrics::new(sc_observability::BuildInfo {
            version: "test",
            commit: "test",
            test_clock: true,
        })
        .expect("metrics");
        let counter = |name| {
            Arc::new(
                metrics
                    .counter::<sc_observability::NoLabels>(name)
                    .expect("counter"),
            )
        };
        let feed = Arc::new(PgChangeFeed::new(
            PgFeedReader::new(api.clone(), &config),
            notifier.clone(),
            Arc::new(ManualClock::new(START)),
            shutdown.clone(),
            counter("feed_missed_wake_total"),
            counter("feed_resync_required_total"),
            &config,
        ));
        Env {
            db,
            api,
            feed,
            notifier,
            shutdown,
        }
    }

    /// `count` accounts with a feed row and no node, so every read finds nothing and wants to park.
    async fn principals(&self, count: usize) -> Vec<Principal> {
        let mut conn = self.db.conn_as(MIGRATOR).await;
        let mut principals = Vec::new();
        for _ in 0..count {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO accounts (id, state, created_at, display_name, role) \
                 VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
            )
            .bind(id)
            .bind(START.as_unix_micros())
            .execute(&mut conn)
            .await
            .expect("insert account");
            sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, $2)")
                .bind(id)
                .bind(EPOCH)
                .execute(&mut conn)
                .await
                .expect("insert feed row");
            principals.push(principal(id));
        }
        conn.close().await.expect("close owner connection");
        principals
    }

    fn open(&self, who: &Principal) -> Held {
        let (feed, who) = (self.feed.clone(), who.clone());
        tokio::spawn(async move { feed.read(&who, &Valid, at_start(), Some(WAIT), None).await })
    }

    /// `reads` reads of each principal, in waves; returns once every read above the cap has been refused.
    async fn open_all(&self, principals: &[Principal], reads: usize) -> Vec<Held> {
        let mut held: Vec<Held> = Vec::new();
        let per_wave = (WAVE / reads).max(1);
        for (wave, chunk) in principals.chunks(per_wave).enumerate() {
            for who in chunk {
                held.extend((0..reads).map(|_| self.open(who)));
            }
            let parked = (wave * per_wave + chunk.len()) * reads.min(CAP);
            let refused = held.len() - parked;
            eventually("the reads above the cap are refused", || {
                held.iter().filter(|read| read.is_finished()).count() >= refused
            })
            .await;
        }
        held
    }
}

fn alive_tasks() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

/// Joins the finished reads, which must all be refusals, and returns the ones still parked.
async fn refusals_and_parked(held: Vec<Held>) -> (usize, Vec<Held>) {
    let (mut refused, mut parked) = (0, Vec::new());
    for read in held {
        if read.is_finished() {
            let result = read.await.expect("read task");
            assert!(
                matches!(result, Err(ChangeFeedError::TooManyHolds { retry_after })
                    if retry_after == PROPOSED_HOLD_RETRY_AFTER),
                "{result:?}"
            );
            refused += 1;
        } else {
            parked.push(read);
        }
    }
    (refused, parked)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn thousand_reads_of_one_principal_and_ten_thousand_of_a_hundred_stay_at_four_each_in_bounded_memory()
 {
    // Arrange
    let env = Env::new().await;
    let one = env.principals(1).await;
    let hundred = env.principals(100).await;
    let bystander = env.principals(1).await.remove(0);
    let other = env.open(&bystander);
    eventually("the bystander's read parked", || {
        env.feed.holds_of(bystander.session_or_device_id) == 1
    })
    .await;
    let baseline_connections = env.db.open_connections().await;
    let baseline_tasks = alive_tasks();
    let baseline_rss = resident_bytes();
    let mut most_connections = baseline_connections;

    // Act
    let mut held = env.open_all(&one, 1_000).await;
    most_connections = most_connections.max(env.db.open_connections().await);
    let rss_one = resident_bytes();
    let tasks_one = alive_tasks();
    let holds_one = env.feed.holds_of(one[0].session_or_device_id);
    let held_hundred = env.open_all(&hundred, 100).await;
    most_connections = most_connections.max(env.db.open_connections().await);
    let rss_hundred = resident_bytes();
    let tasks_hundred = alive_tasks();
    let holds_hundred: Vec<_> = hundred
        .iter()
        .map(|who| env.feed.holds_of(who.session_or_device_id))
        .collect();
    let in_use = env.api.stats().pool_in_use;
    let bystander_holds = env.feed.holds_of(bystander.session_or_device_id);
    held.extend(held_hundred);
    let (refused, parked) = refusals_and_parked(held).await;
    env.shutdown.cancel();
    let mut ended = Vec::new();
    for read in parked.into_iter().chain([other]) {
        ended.push(read.await.expect("read task"));
    }

    // Assert
    println!(
        "ST-05-3: rss +{} B after 1,000 reads and +{} B after 10,000 more, tasks {baseline_tasks} -> {tasks_one} -> {tasks_hundred}, connections {baseline_connections} -> {most_connections}",
        rss_one.saturating_sub(baseline_rss),
        rss_hundred.saturating_sub(baseline_rss),
    );
    assert_eq!(holds_one, CAP as u64);
    assert!(holds_hundred.iter().all(|holds| *holds == CAP as u64));
    assert_eq!(refused, (1_000 - CAP) + 100 * (100 - CAP));
    assert_eq!(ended.len(), CAP + 100 * CAP + 1);
    assert_eq!(bystander_holds, 1, "another principal is unaffected");
    assert!(
        ended
            .iter()
            .all(|result| result.as_ref().is_ok_and(|page| page.items.is_empty()))
    );
    assert!(rss_one.saturating_sub(baseline_rss) <= RSS_BUDGET_BYTES);
    assert!(rss_hundred.saturating_sub(baseline_rss) <= RSS_BUDGET_BYTES);
    assert_eq!(in_use, 0, "parked reads hold no connection");
    assert!(most_connections <= baseline_connections + CONNECTIONS_ABOVE_BASELINE);
    assert!(tasks_one <= baseline_tasks + CAP + TASKS_ABOVE_PARKED);
    assert!(tasks_hundred <= baseline_tasks + 100 * CAP + CAP + TASKS_ABOVE_PARKED);
    drop((env.feed, env.notifier, env.api));
    assert_eq!(env.db.finish().await, 0, "connections left open");
}
