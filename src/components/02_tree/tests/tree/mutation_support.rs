//! An account with its root folder and feed row, and `PgTreeService` wired to the real 05 stamp and notifier on
//! the same database. Probes run as the superuser, so they see every row whatever the scope.

use std::sync::{Arc, Mutex, PoisonError};

use contracts::{AccountGuard, AccountGuardError, async_trait};
use sc_db::DbTx;
use sqlx::PgConnection;
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_tree::PgTreeService;
use uuid::Uuid;

use crate::support::{Fixture, account_with_root, node};

/// The `AccountGuard` port backed at its boundary (the real guard is wired by E-13/c16): it reads the account
/// state `FOR SHARE` as the identity guard does, and records whether the transaction had written or locked
/// anything before it ran.
#[derive(Default)]
pub struct StateGuard {
    first_statement: Mutex<Vec<bool>>,
}

impl StateGuard {
    /// One entry per call: `true` when no row had been written or locked in the transaction before the guard.
    pub fn calls(&self) -> Vec<bool> {
        self.first_statement
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl AccountGuard for StateGuard {
    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountGuardError> {
        let unexpected = |error: sqlx::Error| AccountGuardError::Unexpected(Box::new(error));
        let clean: bool = sqlx::query_scalar("SELECT pg_current_xact_id_if_assigned() IS NULL")
            .fetch_one(tx.conn())
            .await
            .map_err(unexpected)?;
        self.first_statement
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(clean);
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM accounts WHERE id = $1 FOR SHARE")
                .bind(account)
                .fetch_optional(tx.conn())
                .await
                .map_err(unexpected)?;
        match state.as_deref() {
            Some("active") => Ok(()),
            _ => Err(AccountGuardError::AccountNotActive),
        }
    }
}

pub struct Tree {
    pub fixture: Fixture,
    pub service: PgTreeService,
    pub guard: Arc<StateGuard>,
    pub notifier: Arc<InProcessNotifier>,
    pub account: Uuid,
    pub root: Uuid,
}

impl Tree {
    pub async fn new() -> Tree {
        let fixture = Fixture::new().await;
        let account = Uuid::new_v4();
        let mut owner = fixture.owner_conn().await;
        let root = account_with_root(&mut owner, account).await;
        feed_row(&mut owner, account).await;
        close(owner).await;
        let guard = Arc::new(StateGuard::default());
        let notifier = InProcessNotifier::new();
        let service = PgTreeService::new(
            fixture.api.clone(),
            guard.clone(),
            Arc::new(PgFeedStamp),
            notifier.clone(),
            sc_testdb::clock_at_start(),
        );
        Tree {
            fixture,
            service,
            guard,
            notifier,
            account,
            root,
        }
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
            fixture, service, ..
        } = self;
        drop(service);
        fixture.finish().await;
    }
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
