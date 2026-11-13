//! The "via 02" rig: one database cloned from the migrated template, an account with its root folder, feed row
//! and quota row, `PgTreeService` wired as 02's own tests wire it (real 05 stamp and notifier, real 07a ledger,
//! the boundary-backed `sunnycloud_testkit::StateGuard` and the real 04 `GcQueue`), and the real feed (`PgFeedReader`,
//! `PgChangeFeed`) on the same pool.
//! Fixtures and probes run as the schema owner or the superuser.

use std::cell::Cell;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use contracts::{
    ChangeFeed, ChangeFeedError, ChangePage, CommitActor, Cursor, EntryResult, FeedNotifier,
    NewRevision, Principal, PrincipalKind, PrincipalRevalidator, PublishEntry, PublishResult,
    RevalidatorError, Role, Seq, Target, TreeCommit, TreeCommitError, async_trait,
};
use sc_clock::{CancellationToken, Clock};
use sc_config::Secret;
use sc_ids::{FileHash, Hash32};
use sc_observability::{BuildInfo, Counter, Metrics, NoLabels};
use sc_testdb::{MIGRATOR, START, TestDb, statements};
use sqlx::{Connection, PgConnection};
use sunnycloud_blobstore::{BlobstoreConfig, GcQueue};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{
    FeedConfig, InProcessNotifier, PgChangeFeed, PgFeedReader, PgFeedStamp,
};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::PgTreeService;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::park_clock::ParkClock;

pub const EPOCH: i64 = 1;
const DEFAULT_QUOTA: i64 = 1 << 40;
const SIZE: i64 = 100;
const DAY: Duration = Duration::from_secs(86_400);

tokio::task_local! {
    /// The `seq` the notifier was woken with inside the current task; each publish helper scopes it.
    pub static NOTIFIED: Cell<Option<Seq>>;
}

pub fn cursor(seq: Seq) -> Cursor {
    Cursor { epoch: EPOCH, seq }
}

pub fn feed_config(page_limit: u64) -> FeedConfig {
    FeedConfig {
        longpoll_wait_max: Duration::from_secs(30),
        longpoll_auth_recheck: Duration::from_secs(2),
        feed_safety_recheck: Duration::from_secs(10),
        feed_page_limit_default: page_limit,
        feed_page_limit_max: None,
        max_holds_per_principal: 4,
    }
}

/// The real in-process notifier, observed: each wake is marked in the statement log, kept in [`NOTIFIED`] and
/// timed (the start of the server-side wake latency).
pub struct ProbeNotifier {
    inner: Arc<InProcessNotifier>,
    sent: Arc<Mutex<Vec<(Seq, Instant)>>>,
}

impl FeedNotifier for ProbeNotifier {
    fn notify(&self, account: Uuid, seq: Seq) {
        statements::mark("notify");
        // Every publish of this binary runs in a `NOTIFIED` scope (`Rig::publish`, `commit`).
        NOTIFIED.with(|cell| cell.set(Some(seq)));
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((seq, Instant::now()));
        self.inner.notify(account, seq);
    }
}

/// The blobstore settings the `GcQueue` reads (retention 7 days); the feed tests write no blob files.
fn blobstore_config() -> BlobstoreConfig {
    BlobstoreConfig {
        blob_root: None,
        retention: 7 * DAY,
        orphan_sweep_age: DAY,
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: 0,
        blob_free_reserve_percent: 100,
        gc_interval: Duration::from_secs(60),
        gc_batch: 100,
        gc_claim_lease: Duration::from_secs(60),
        gc_cycle_budget: Duration::from_secs(60),
        scrub_interval: Duration::from_secs(3600),
        scrub_cycle_budget: Duration::from_secs(60),
        scrub_max_rate: 1 << 20,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: false,
        blob_read_ahead_chunks: 1,
        blob_io_threads: None,
        master_key_file: Secret::new("unused".to_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

fn metric_counter(name: &'static str) -> Arc<Counter<NoLabels>> {
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics registry");
    Arc::new(metrics.counter(name).expect("counter"))
}

pub struct Rig {
    pub db: TestDb,
    pub service: Arc<PgTreeService>,
    pub reader: PgFeedReader,
    pub feed: Arc<PgChangeFeed>,
    pub clock: Arc<ParkClock>,
    pub shutdown: CancellationToken,
    pub account: Uuid,
    pub root: Uuid,
    sent: Arc<Mutex<Vec<(Seq, Instant)>>>,
}

impl Rig {
    /// The held reads run on `clock`; `page_limit` is the feed's default page size.
    pub async fn new(clock: Arc<dyn Clock>, page_limit: u64) -> Rig {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let account = Uuid::new_v4();
        let mut owner = db.conn_as(MIGRATOR).await;
        let root = account_rows(&mut owner, account).await;
        owner.close().await.expect("close owner connection");
        let notifier = InProcessNotifier::new();
        let sent = Arc::default();
        let start_clock = sc_testdb::clock_at_start();
        let queue =
            GcQueue::new(&blobstore_config(), api.clone(), start_clock.clone()).expect("gc queue");
        let service = Arc::new(PgTreeService::new(
            api.clone(),
            Arc::new(StateGuard::default()),
            Arc::new(PgQuotaLedger::new(QuotaConfig {
                default_quota_bytes: DEFAULT_QUOTA,
            })),
            Arc::new(queue),
            Arc::new(PgFeedStamp),
            Arc::new(ProbeNotifier {
                inner: notifier.clone(),
                sent: Arc::clone(&sent),
            }),
            start_clock,
        ));
        let config = feed_config(page_limit);
        let clock = ParkClock::over(clock);
        let shutdown = CancellationToken::new();
        let feed = Arc::new(PgChangeFeed::new(
            PgFeedReader::new(api.clone(), &config),
            notifier.clone(),
            clock.clone(),
            shutdown.clone(),
            metric_counter("feed_missed_wake_total"),
            metric_counter("feed_resync_required_total"),
            &config,
        ));
        Rig {
            db,
            service,
            reader: PgFeedReader::new(api, &config),
            feed,
            clock,
            shutdown,
            account,
            root,
            sent,
        }
    }

    /// `count` committed blob rows of the account, inserted in one statement.
    pub async fn blobs(&self, count: usize) -> Vec<Uuid> {
        let ids: Vec<Uuid> = (0..count).map(|_| Uuid::new_v4()).collect();
        let mut admin = self.db.admin().await;
        sqlx::query(
            "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
             SELECT id, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz FROM unnest($1::uuid[]) AS id",
        )
        .bind(&ids)
        .bind(self.account)
        .execute(&mut admin)
        .await
        .expect("insert blobs");
        admin.close().await.expect("close admin connection");
        ids
    }

    /// One publish of one entry through the real `TreeCommit`, by device `device`.
    pub async fn publish(
        &self,
        device: &str,
        entry: PublishEntry,
    ) -> Result<PublishResult, TreeCommitError> {
        let actor = CommitActor {
            account_id: self.account,
            device_label: device.to_owned(),
        };
        NOTIFIED
            .scope(
                Cell::new(None),
                self.service.publish(actor, vec![entry], Vec::new(), START),
            )
            .await
    }

    /// A held read of device B from `at` with `wait`, on its own task; returns with the instant it returned.
    pub fn hold(&self, at: Cursor, wait: Duration) -> Held {
        let feed = self.feed.clone();
        let device_b = principal(self.account, PrincipalKind::Device);
        tokio::spawn(async move {
            let page = feed
                .read(&device_b, &StillValid, at, Some(wait), None)
                .await;
            (page, Instant::now())
        })
    }

    /// The account's feed counter, read by the superuser.
    pub async fn counter(&self) -> i64 {
        self.db
            .admin_count(&format!(
                "SELECT seq FROM account_feeds WHERE account_id = '{}'",
                self.account
            ))
            .await
    }

    /// The `seq` of every notify so far, with the instant it was sent.
    pub fn notifies(&self) -> Vec<(Seq, Instant)> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub async fn finish(self) {
        let Rig {
            db,
            service,
            reader,
            feed,
            shutdown,
            ..
        } = self;
        shutdown.cancel();
        drop((service, reader, feed));
        assert_eq!(db.finish().await, 0, "connections left open");
    }
}

pub type Held = JoinHandle<(Result<ChangePage, ChangeFeedError>, Instant)>;

/// A principal that stays valid; no test here holds past `longpoll_auth_recheck`.
struct StillValid;

#[async_trait]
impl PrincipalRevalidator for StillValid {
    async fn revalidate(&self, _principal: &Principal) -> Result<bool, RevalidatorError> {
        Ok(true)
    }
}

/// One commit through the real `publish`: its node, its revision and the `seq` its notify carried.
pub async fn commit(
    service: &PgTreeService,
    account: Uuid,
    entry: PublishEntry,
) -> (Uuid, Uuid, Seq) {
    NOTIFIED
        .scope(Cell::new(None), async {
            let actor = CommitActor {
                account_id: account,
                device_label: "laptop".to_owned(),
            };
            let result = service
                .publish(actor, vec![entry], Vec::new(), START)
                .await
                .expect("publish");
            let (node_id, revision_id) = published(&result);
            let seq = NOTIFIED.with(Cell::get).expect("notified after COMMIT");
            (node_id, revision_id, seq)
        })
        .await
}

/// The node and revision of a one-entry publish that published.
pub fn published(result: &PublishResult) -> (Uuid, Uuid) {
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay: {result:?}");
    };
    let [
        EntryResult::Published {
            node_id,
            revision_id,
            ..
        },
    ] = results[..]
    else {
        panic!("not published: {results:?}");
    };
    (node_id, revision_id)
}

pub fn principal(account: Uuid, kind: PrincipalKind) -> Principal {
    Principal {
        account_id: account,
        kind,
        session_or_device_id: Uuid::new_v4(),
        role: Role::User,
        label: "device-b".to_owned(),
    }
}

/// A content hash of its own for every different `seed`.
fn hash_of(seed: Uuid) -> FileHash {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(seed.as_bytes());
    FileHash::from_bytes(bytes)
}

fn revision(blob_id: Uuid) -> NewRevision {
    let revision_id = Uuid::new_v4();
    NewRevision {
        revision_id,
        blob_id,
        size: SIZE,
        content_hash: hash_of(revision_id),
        blocklist: vec![Hash32::from_bytes([3; 32])],
        media_type: Some("text/plain".to_owned()),
        client_mtime: None,
        client_meta: None,
        reserved_bytes: 0,
        import_hint: false,
    }
}

pub fn new_file(parent_id: Uuid, name: &str, blob_id: Uuid) -> PublishEntry {
    PublishEntry {
        target: Target::New {
            parent_id,
            name: name.to_owned(),
        },
        revision: revision(blob_id),
        vault: false,
    }
}

pub fn existing(node_id: Uuid, base_revision: Uuid, blob_id: Uuid) -> PublishEntry {
    PublishEntry {
        target: Target::Existing {
            node_id,
            base_revision: Some(base_revision),
        },
        revision: revision(blob_id),
        vault: false,
    }
}

/// The account, its root folder, its feed row and its quota row; returns the root.
async fn account_rows(conn: &mut PgConnection, account: Uuid) -> Uuid {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(account)
    .bind(START.as_unix_micros())
    .execute(&mut *conn)
    .await
    .expect("insert account");
    let root = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, account_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, 'folder', '', '', 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(root)
    .bind(account)
    .execute(&mut *conn)
    .await
    .expect("insert root");
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, $2)")
        .bind(account)
        .bind(EPOCH)
        .execute(&mut *conn)
        .await
        .expect("insert feed row");
    sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
        .bind(account)
        .execute(&mut *conn)
        .await
        .expect("insert quota row");
    root
}

/// Lets spawned tasks run until they block again; a negative check ("still parked") is made after it.
pub async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}
