//! PT-05-3 and PT-05-4 run on REF-SRV only (`cargo test -- --ignored`); `PT_05_3_SECONDS` and `PT_05_4_SECONDS`
//! shorten the runs for a smoke check, the thresholds apply unchanged. They use the system clock and real waits.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use contracts::{ChangeFeed, Cursor, FeedNotifier};
use sc_clock::{CancellationToken, SystemClock};
use sc_db::{Db, DbConfig, DbRole};
use sqlx::Connection;
use sunnycloud_sync_feed::{FeedConfig, InProcessNotifier, PgChangeFeed, PgFeedReader};
use uuid::Uuid;

use crate::hold_support::{Acct, eventually, missed_wake_counter, principal, valid};
use crate::read_support::{EPOCH, feed_config};
use crate::support::{Fixture, account_with_feed, root_node};

fn seconds(variable: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(variable)
            .ok()
            .map_or(default, |text| text.parse().expect("a number of seconds")),
    )
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

struct Load {
    fixture: Fixture,
    feed: Arc<PgChangeFeed>,
    feed_db: Db,
    notifier: Arc<InProcessNotifier>,
    missed: Arc<sc_observability::Counter<sc_observability::NoLabels>>,
    stop: CancellationToken,
}

impl Load {
    async fn new(stamp_pool: u32, feed_pool: u32) -> Load {
        let fixture = Fixture::with_pool_max(stamp_pool).await;
        let feed_db = Db::connect(
            &DbConfig {
                db_pool_max: feed_pool,
                ..fixture.db.config()
            },
            DbRole::Api,
        )
        .await
        .expect("connect the feed pool");
        let config: FeedConfig = feed_config(500, None);
        let notifier = InProcessNotifier::new();
        let stop = CancellationToken::new();
        let missed = missed_wake_counter();
        let feed = Arc::new(PgChangeFeed::new(
            PgFeedReader::new(feed_db.clone(), &config),
            notifier.clone(),
            Arc::new(SystemClock::new()),
            stop.clone(),
            missed.clone(),
            &config,
        ));
        Load {
            fixture,
            feed,
            feed_db,
            notifier,
            missed,
            stop,
        }
    }

    async fn accounts(&self, count: usize) -> Vec<Acct> {
        let mut conn = self.fixture.owner_conn().await;
        let mut accounts = Vec::new();
        for _ in 0..count {
            let id = Uuid::new_v4();
            account_with_feed(&mut conn, id, EPOCH).await;
            let root = root_node(&mut conn, id).await;
            accounts.push(Acct::new(id, root));
        }
        conn.close().await.expect("close owner connection");
        accounts
    }

    /// A holder re-polls with the cursor of the last page until `stop`; `on_page` sees each non-empty page.
    fn holder(
        &self,
        account: Uuid,
        on_page: impl Fn(Uuid, Instant) + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        let feed = self.feed.clone();
        tokio::spawn(async move {
            let who = principal(account);
            let revalidator = valid();
            let mut at = Cursor {
                epoch: EPOCH,
                seq: 0,
            };
            loop {
                let page = feed
                    .read(
                        &who,
                        revalidator.as_ref(),
                        at,
                        Some(Duration::from_secs(30)),
                        None,
                    )
                    .await
                    .expect("page");
                if page.items.is_empty() {
                    return;
                }
                on_page(account, Instant::now());
                at = page.next_cursor;
            }
        })
    }

    async fn finish(self, holders: Vec<tokio::task::JoinHandle<()>>) {
        self.stop.cancel();
        for holder in holders {
            holder.await.expect("holder task");
        }
        let Load {
            fixture,
            feed,
            feed_db,
            notifier,
            ..
        } = self;
        drop((feed, feed_db, notifier));
        fixture.finish().await;
    }
}

impl Load {
    /// Waits until `holders` reads are parked: registered with the notifier and every connection back in the pool.
    async fn parked(&self, accounts: &[Acct], holders: usize) {
        eventually("every holder parked", || {
            let registered: usize = accounts
                .iter()
                .map(|account| self.notifier.waiters(account.id))
                .sum();
            registered == holders && self.feed_db.stats().pool_in_use == 0
        })
        .await;
    }

    /// A stamp and notify on each account every 2 s for `duration`; returns the most server connections seen.
    async fn stamp_rounds(&self, accounts: &[Acct], duration: Duration) -> i64 {
        let deadline = Instant::now() + duration;
        let mut most = 0;
        while Instant::now() < deadline {
            for account in accounts {
                let seq = account.commit_on(&self.fixture.api).await;
                self.notifier.notify(account.id, seq);
            }
            most = most.max(self.fixture.db.open_connections().await);
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        most
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-05-3 is measured on REF-SRV only"]
async fn pt_05_3_hundred_and_a_thousand_holders_use_few_connections_and_little_memory() {
    // Arrange
    let load = Load::new(1, 4).await;
    let accounts = load.accounts(10).await;
    let duration = seconds("PT_05_3_SECONDS", 120);
    let mut tasks = Vec::new();
    let parked = |count: usize, tasks: &mut Vec<_>| {
        for index in tasks.len()..tasks.len() + count {
            tasks.push(load.holder(accounts[index % accounts.len()].id, |_, _| {}));
        }
    };

    // Act
    parked(100, &mut tasks);
    load.parked(&accounts, 100).await;
    let at_hundred = resident_bytes();
    let connections_at_hundred = load.stamp_rounds(&accounts, duration).await;
    parked(900, &mut tasks);
    load.parked(&accounts, 1_000).await;
    let per_holder = resident_bytes().saturating_sub(at_hundred) / 900;
    let connections_at_thousand = load.stamp_rounds(&accounts, duration).await;

    // Assert
    println!(
        "PT-05-3: connections {connections_at_hundred} / {connections_at_thousand}, {per_holder} B per holder"
    );
    assert!(connections_at_hundred <= 5 && connections_at_thousand <= 5);
    assert!(per_holder <= 16 * 1024, "{per_holder} bytes per holder");
    assert_eq!(load.missed.get(sc_observability::NoLabels), 0);
    load.finish(tasks).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-05-4 is measured on REF-SRV only"]
async fn pt_05_4_wake_latency_from_notify_to_return_meets_the_targets_under_background_load() {
    // Arrange
    let load = Load::new(100, 10).await;
    let accounts = load.accounts(100).await;
    let background = load.accounts(1).await;
    let notified_at: Arc<Vec<AtomicU64>> =
        Arc::new((0..accounts.len()).map(|_| AtomicU64::new(0)).collect());
    let origin = Instant::now();
    let latencies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let index_of: Arc<std::collections::HashMap<Uuid, usize>> = Arc::new(
        accounts
            .iter()
            .enumerate()
            .map(|(index, account)| (account.id, index))
            .collect(),
    );
    let tasks: Vec<_> = accounts
        .iter()
        .map(|account| {
            let (notified_at, latencies, index_of) =
                (notified_at.clone(), latencies.clone(), index_of.clone());
            load.holder(account.id, move |id, returned| {
                let sent = notified_at[index_of[&id]].load(Ordering::SeqCst);
                let returned =
                    u64::try_from(returned.duration_since(origin).as_nanos()).unwrap_or(u64::MAX);
                latencies
                    .lock()
                    .expect("latency list")
                    .push(Duration::from_nanos(returned.saturating_sub(sent)));
            })
        })
        .collect();
    let running = Arc::new(AtomicBool::new(true));
    let streams: Vec<_> = (0..100)
        .map(|_| {
            let (load_db, running, account) = (
                load.fixture.api.clone(),
                running.clone(),
                background[0].clone(),
            );
            tokio::spawn(async move {
                while running.load(Ordering::SeqCst) {
                    account.commit_on(&load_db).await;
                }
            })
        })
        .collect();
    let deadline = Instant::now() + seconds("PT_05_4_SECONDS", 300);

    // Act
    while Instant::now() < deadline {
        for (index, account) in accounts.iter().enumerate() {
            let seq = account.commit_on(&load.fixture.api).await;
            let sent = u64::try_from(origin.elapsed().as_nanos()).unwrap_or(u64::MAX);
            notified_at[index].store(sent, Ordering::SeqCst);
            load.notifier.notify(account.id, seq);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    running.store(false, Ordering::SeqCst);
    for stream in streams {
        stream.await.expect("background stream");
    }

    // Assert
    let mut sorted = latencies.lock().expect("latency list").clone();
    sorted.sort_unstable();
    let at = |percent: usize| sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)];
    println!(
        "PT-05-4: {} wakes, p95 {:?}, p99 {:?}",
        sorted.len(),
        at(95),
        at(99)
    );
    assert!(at(95) <= Duration::from_millis(50), "p95 {:?}", at(95));
    assert!(at(99) <= Duration::from_millis(150), "p99 {:?}", at(99));
    load.finish(tasks).await;
}
