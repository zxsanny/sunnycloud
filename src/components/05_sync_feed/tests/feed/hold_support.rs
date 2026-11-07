//! Rig of the hold tests: real PostgreSQL 18, real stamps, a probe clock that moves only on `tick`, and scripted
//! revalidators. Waiting is on channels, joins and the probe clock's sleep counter, never on elapsed time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use contracts::{
    ChangeFeed, ChangeFeedError, ChangePage, Cursor, FeedNotifier, FeedStamp, Principal,
    PrincipalKind, PrincipalRevalidator, RevalidatorError, Role, Seq, TouchedNodes, async_trait,
};
use sc_clock::{CancellationToken, MonotonicInstant};
use sc_db::{Db, Scope};
use sc_observability::{BuildInfo, Counter, Metrics, NoLabels};
use sqlx::Connection;
use sunnycloud_sync_feed::{
    FeedConfig, InProcessNotifier, PgChangeFeed, PgFeedReader, PgFeedStamp,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::probe_clock::ProbeClock;
use crate::read_support::{EPOCH, feed_config};
use crate::support::{Fixture, START, account_with_feed, insert_folders, root_node};

pub const HANG_GUARD: Duration = Duration::from_secs(60);

pub fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

pub fn cursor(seq: Seq) -> Cursor {
    Cursor { epoch: EPOCH, seq }
}

/// The long-poll section with millisecond values; the caller overrides what it tests.
pub fn config(auth_ms: u64, safety_ms: u64) -> FeedConfig {
    FeedConfig {
        longpoll_auth_recheck: ms(auth_ms),
        feed_safety_recheck: ms(safety_ms),
        ..feed_config(500, None)
    }
}

pub fn missed_wake_counter() -> Arc<Counter<NoLabels>> {
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics registry");
    Arc::new(metrics.counter("feed_missed_wake_total").expect("counter"))
}

pub struct Rig {
    pub fixture: Fixture,
    pub clock: Arc<ProbeClock>,
    pub notifier: Arc<InProcessNotifier>,
    pub shutdown: CancellationToken,
    pub missed: Arc<Counter<NoLabels>>,
    pub feed: Arc<PgChangeFeed>,
    wait_max: Duration,
}

impl Rig {
    pub async fn new(config: &FeedConfig, pool_max: u32) -> Rig {
        let fixture = Fixture::with_pool_max(pool_max).await;
        let db = fixture.api.clone();
        Rig::over(fixture, db, config)
    }

    /// The feed reads through `db`; the fixture keeps its own pool for stamps.
    pub fn over(fixture: Fixture, db: Db, config: &FeedConfig) -> Rig {
        let clock = ProbeClock::new(START);
        let notifier = InProcessNotifier::new();
        let shutdown = CancellationToken::new();
        let missed = missed_wake_counter();
        let feed = Arc::new(PgChangeFeed::new(
            PgFeedReader::new(db, config),
            notifier.clone(),
            clock.clone(),
            shutdown.clone(),
            missed.clone(),
            config,
        ));
        Rig {
            fixture,
            clock,
            notifier,
            shutdown,
            missed,
            feed,
            wait_max: config.longpoll_wait_max,
        }
    }

    pub fn feed_wait_max(&self) -> Duration {
        self.wait_max
    }

    pub async fn account(&self) -> Acct {
        let mut conn = self.fixture.owner_conn().await;
        let id = Uuid::new_v4();
        account_with_feed(&mut conn, id, EPOCH).await;
        let root = root_node(&mut conn, id).await;
        conn.close().await.expect("close owner connection");
        Acct::new(id, root)
    }

    pub async fn commit(&self, account: &Acct) -> Seq {
        account.commit_on(&self.fixture.api).await
    }

    /// A read without wait: the immediate path.
    pub async fn feed_read_now(
        &self,
        principal: &Principal,
        at: Cursor,
    ) -> Result<ChangePage, ChangeFeedError> {
        self.feed
            .read(principal, &Scripted::valid(), at, None, None)
            .await
    }

    /// A node of the account that no stamp has touched yet (seq 0).
    pub async fn created_node(&self, account: &Acct) -> Uuid {
        let mut conn = self.fixture.owner_conn().await;
        let nodes = insert_folders(&mut conn, account.id, account.root, 1)
            .await
            .expect("insert node");
        conn.close().await.expect("close owner connection");
        nodes[0]
    }

    pub async fn commit_and_notify(&self, account: &Acct) -> Seq {
        let seq = self.commit(account).await;
        self.notifier.notify(account.id, seq);
        seq
    }

    pub fn read(
        &self,
        principal: &Principal,
        revalidator: Arc<dyn PrincipalRevalidator>,
        at: Cursor,
        wait: Duration,
    ) -> Held {
        let feed = self.feed.clone();
        let principal = principal.clone();
        tokio::spawn(async move {
            feed.read(&principal, revalidator.as_ref(), at, Some(wait), None)
                .await
        })
    }

    pub async fn finish(self) {
        let Rig {
            fixture,
            feed,
            notifier,
            ..
        } = self;
        drop((feed, notifier));
        fixture.finish().await;
    }
}

pub type Held = JoinHandle<Result<ChangePage, ChangeFeedError>>;

pub async fn outcome(held: Held) -> Result<ChangePage, ChangeFeedError> {
    tokio::time::timeout(HANG_GUARD, held)
        .await
        .expect("the hold never ended")
        .expect("hold task")
}

pub fn principal(account: Uuid) -> Principal {
    Principal {
        account_id: account,
        kind: PrincipalKind::Session,
        session_or_device_id: Uuid::new_v4(),
        role: Role::User,
        label: "test".to_owned(),
    }
}

pub fn valid() -> Arc<dyn PrincipalRevalidator> {
    Arc::new(Scripted::valid())
}

/// A revalidator that answers `true` until its flag is cleared, or until the probe clock reaches `revoke_at`.
pub struct Scripted {
    valid: AtomicBool,
    revoke_at: Option<(Arc<ProbeClock>, MonotonicInstant)>,
    pub calls: AtomicUsize,
}

impl Scripted {
    pub fn valid() -> Self {
        Scripted {
            valid: AtomicBool::new(true),
            revoke_at: None,
            calls: AtomicUsize::new(0),
        }
    }

    pub fn revoked_at(clock: &Arc<ProbeClock>, after: Duration) -> Self {
        use sc_clock::Clock;
        let at = clock
            .monotonic()
            .checked_add(after)
            .expect("instant in range");
        Scripted {
            revoke_at: Some((clock.clone(), at)),
            ..Scripted::valid()
        }
    }
}

#[async_trait]
impl PrincipalRevalidator for Scripted {
    async fn revalidate(&self, _principal: &Principal) -> Result<bool, RevalidatorError> {
        use sc_clock::Clock;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .revoke_at
            .as_ref()
            .is_some_and(|(clock, at)| clock.monotonic() >= *at);
        Ok(self.valid.load(Ordering::SeqCst) && !revoked)
    }
}

#[derive(Clone)]
pub struct Acct {
    pub id: Uuid,
    root: Uuid,
}

impl Acct {
    pub fn new(id: Uuid, root: Uuid) -> Acct {
        Acct { id, root }
    }

    /// One new node stamped in one committed transaction, as 02's publish does; returns the seq. Notifying is the
    /// caller's step, after the commit.
    pub async fn commit_on(&self, db: &Db) -> Seq {
        db.run(Scope::Account(self.id), async |tx| {
            let node = insert_folders(tx.conn(), self.id, self.root, 1)
                .await
                .map_err(|error| ChangeFeedError::Unexpected(Box::new(error)))?;
            PgFeedStamp
                .stamp(tx, self.id, TouchedNodes(node))
                .await
                .map_err(|error| ChangeFeedError::Unexpected(Box::new(error)))
        })
        .await
        .expect("stamp transaction")
    }
}

/// Lets spawned holds run until they block again; a negative check ("still parked") is made after it.
pub async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// Polls `condition` until it holds; the hang guard fails the test instead of hanging it.
pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let polled = async {
        while !condition() {
            tokio::time::sleep(ms(1)).await;
        }
    };
    tokio::time::timeout(HANG_GUARD, polled)
        .await
        .unwrap_or_else(|_| panic!("never happened: {what}"));
}
