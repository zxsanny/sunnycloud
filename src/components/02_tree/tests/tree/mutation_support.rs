//! An account with its root folder, feed row and quota row, and `PgTreeService` wired to the real 05 stamp and
//! notifier, the real 07a ledger and the real 04 `GcQueue` on the same database. Probes run as the superuser, so they see every row
//! whatever the scope.

use std::cell::Cell;
use std::sync::Arc;

use contracts::{FeedNotifier, Seq};
use sc_clock::Clock;
use sqlx::PgConnection;
use sunnycloud_blobstore::GcQueue;
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_tree::PgTreeService;
use uuid::Uuid;

use crate::support::{Fixture, account_with_root, node};
use sc_testdb::statements;
use sunnycloud_testkit::StateGuard;

/// The installation default limit of the ledger in these tests: large enough never to refuse.
pub const DEFAULT_QUOTA: i64 = 1 << 40;

tokio::task_local! {
    /// The `seq` the notifier was woken with inside the current task, when a test scopes it.
    pub static NOTIFIED: Cell<Option<Seq>>;
}

/// The real in-process notifier, observed: each wake is marked in the statement log and kept in [`NOTIFIED`].
pub struct ProbeNotifier(pub Arc<InProcessNotifier>);

impl FeedNotifier for ProbeNotifier {
    fn notify(&self, account: Uuid, seq: Seq) {
        statements::mark("notify");
        let _outside_a_scope = NOTIFIED.try_with(|cell| cell.set(Some(seq)));
        self.0.notify(account, seq);
    }
}

pub struct Tree {
    pub fixture: Fixture,
    pub service: Arc<PgTreeService>,
    /// The queue `service` writes (blob root unused by the queue; retention 7 days).
    pub queue: Arc<GcQueue>,
    pub guard: Arc<StateGuard>,
    pub notifier: Arc<InProcessNotifier>,
    pub account: Uuid,
    pub root: Uuid,
}

impl Tree {
    pub async fn new() -> Tree {
        Tree::with_clock(sc_testdb::clock_at_start()).await
    }

    /// The tree on a clock of the test's choice (conflict-copy names read it).
    pub async fn with_clock(clock: Arc<dyn Clock>) -> Tree {
        let fixture = Fixture::new().await;
        let account = Uuid::new_v4();
        let mut owner = fixture.owner_conn().await;
        let root = account_rows(&mut owner, account).await;
        close(owner).await;
        let guard = Arc::new(StateGuard::default());
        let notifier = InProcessNotifier::new();
        let blobstore = crate::lifecycle_support::config(&std::env::temp_dir());
        let queue = Arc::new(
            GcQueue::new(&blobstore, fixture.api.clone(), clock.clone()).expect("gc queue"),
        );
        let service = Arc::new(PgTreeService::new(
            fixture.api.clone(),
            guard.clone(),
            Arc::new(PgQuotaLedger::new(QuotaConfig {
                default_quota_bytes: DEFAULT_QUOTA,
            })),
            queue.clone(),
            Arc::new(PgFeedStamp),
            Arc::new(ProbeNotifier(notifier.clone())),
            clock,
        ));
        Tree {
            fixture,
            service,
            queue,
            guard,
            notifier,
            account,
            root,
        }
    }

    /// One more account on the same database, with its root, feed and quota rows; returns it and its root.
    pub async fn add_account(&self) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let mut owner = self.fixture.owner_conn().await;
        let root = account_rows(&mut owner, account).await;
        close(owner).await;
        (account, root)
    }

    /// The account's feed counter.
    pub async fn seq(&self) -> i64 {
        self.count(&format!(
            "SELECT seq FROM account_feeds WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    /// Node rows of the account, live and tombstoned.
    pub async fn nodes(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    pub async fn count(&self, sql: &str) -> i64 {
        self.fixture.db.admin_count(sql).await
    }

    pub async fn exec(&self, sql: &str) {
        self.fixture.db.admin_exec(sql).await;
    }

    /// A folder row written by the fixture, not through the service.
    pub async fn folder(&self, parent: Uuid, name: &str) -> Uuid {
        let mut owner = self.fixture.owner_conn().await;
        let id = node(&mut owner, self.account, Some(parent), "folder", name)
            .await
            .expect("insert folder");
        close(owner).await;
        id
    }

    pub async fn tombstone(&self, id: Uuid) {
        self.exec(&format!(
            "UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = '{id}'"
        ))
        .await;
    }

    pub async fn finish(self) {
        let Tree {
            fixture,
            service,
            queue,
            ..
        } = self;
        drop((service, queue));
        fixture.finish().await;
    }
}

/// The account, its root, its feed row and its quota row; returns the root.
async fn account_rows(conn: &mut PgConnection, account: Uuid) -> Uuid {
    let root = account_with_root(conn, account).await;
    feed_row(conn, account).await;
    sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
        .bind(account)
        .execute(&mut *conn)
        .await
        .expect("insert quota row");
    root
}

pub async fn feed_row(conn: &mut PgConnection, account: Uuid) {
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)")
        .bind(account)
        .execute(conn)
        .await
        .expect("insert feed row");
}

pub async fn close(conn: PgConnection) {
    use sqlx::Connection;
    conn.close().await.expect("close fixture connection");
}
