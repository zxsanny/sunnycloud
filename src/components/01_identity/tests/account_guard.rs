//! AC-4 and AC-6 of the account administration (IT-01-15, IT-01-16; FT-N-46, NFT-RES-34, NFT-RES-38): the
//! active-admin lock makes a second caller wait, and the guard's shared lock lets an in-flight commit finish
//! before `begin_deletion` while refusing every later one. Waiting is read from `pg_locks`. Real PostgreSQL;
//! the transactions are held open on separate connections of the component's pool.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{AccountAdmin, AccountGuard, AccountGuardError};
use sc_db::{RunError, Scope};
use sqlx::{Connection, PgConnection};
use sunnycloud_identity::IdentityAccounts;
use support::Harness;
use support::START;
use support::accounts::{accounts, until_a_lock_waits, waiting_locks};
use support::fixtures;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

const HELD: Duration = Duration::from_millis(300);
const WAIT_BOUND: Duration = Duration::from_secs(2);

struct Fixture {
    harness: Harness,
    conn: PgConnection,
    admin: Arc<IdentityAccounts>,
}

async fn fixture() -> Fixture {
    let harness = Harness::new().await;
    let conn = harness.fixture_conn().await;
    let admin = accounts(&harness);
    Fixture {
        harness,
        conn,
        admin,
    }
}

impl Fixture {
    async fn account(&mut self, role: &str, state: &str) -> Uuid {
        let id = Uuid::new_v4();
        fixtures::account_in(&mut self.conn, id, role, state, START.as_unix_micros()).await;
        id
    }

    async fn finish(self) {
        let Fixture {
            harness,
            conn,
            admin,
        } = self;
        drop(admin);
        conn.close().await.expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }

    /// A transaction that locks the active admins, reports their ids and stays open until released.
    fn hold_admin_lock(
        &self,
        scope: Uuid,
    ) -> (
        JoinHandle<Vec<Uuid>>,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
    ) {
        let (db, admin) = (self.harness.api.clone(), Arc::clone(&self.admin));
        let (locked_tx, locked_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            db.run(Scope::Account(scope), async |tx| {
                let ids = admin.lock_active_admins(tx).await?;
                locked_tx.send(()).expect("the test is waiting");
                release_rx.await.expect("the test releases the lock");
                Ok::<_, contracts::AccountAdminError>(ids)
            })
            .await
            .expect("lock_active_admins transaction")
        });
        (task, locked_rx, release_tx)
    }
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

#[tokio::test]
async fn the_second_caller_of_lock_active_admins_waits_for_the_first_to_commit() {
    // Arrange
    let mut f = fixture().await;
    let (first_admin, second_admin) = (
        f.account("admin", "active").await,
        f.account("admin", "active").await,
    );
    let disabled_admin = f.account("admin", "disabled").await;
    let member = f.account("user", "active").await;
    let expected = sorted(vec![first_admin, second_admin]);
    let (first, first_locked, release_first) = f.hold_admin_lock(member);
    first_locked.await.expect("the first caller holds the lock");

    // Act
    let (second, second_locked, release_second) = f.hold_admin_lock(member);
    until_a_lock_waits(&mut f.conn).await;
    let waiting_while_held = !second.is_finished();
    release_second.send(()).expect("second not yet released");
    let disabled_row_locked: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE SKIP LOCKED)",
    )
    .bind(disabled_admin)
    .fetch_one(&mut f.conn)
    .await
    .expect("probe the disabled admin row");
    release_first.send(()).expect("first still holds");
    let first_ids = first.await.expect("first");
    second_locked
        .await
        .expect("the second caller got the lock after the first committed");
    let second_ids = second.await.expect("second");

    // Assert
    assert!(
        waiting_while_held,
        "the second caller finished while the first held the lock"
    );
    assert!(!disabled_row_locked, "a disabled admin is not locked");
    assert_eq!(sorted(first_ids), expected);
    assert_eq!(sorted(second_ids), expected);
    f.finish().await;
}

#[tokio::test]
async fn begin_deletion_waits_for_an_in_flight_guard_then_completes_and_later_guards_are_refused() {
    // Arrange
    let mut f = fixture().await;
    let account = f.account("user", "active").await;
    let (db, admin) = (f.harness.api.clone(), Arc::clone(&f.admin));
    let (guarded_tx, guarded_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let holder = tokio::spawn({
        let (db, admin) = (db.clone(), Arc::clone(&admin));
        async move {
            db.run(Scope::Account(account), async |tx| {
                admin.ensure_active(tx, account).await?;
                guarded_tx.send(()).expect("the test is waiting");
                release_rx.await.expect("the test releases the guard");
                Ok::<_, AccountGuardError>(())
            })
            .await
        }
    });
    guarded_rx.await.expect("connection 1 passed the guard");

    // Act
    let started = Instant::now();
    let deletion = tokio::spawn({
        let (db, admin) = (db.clone(), Arc::clone(&admin));
        async move {
            db.run(Scope::Account(account), async |tx| {
                admin.begin_deletion(tx, account).await
            })
            .await
            .expect("begin_deletion");
            started.elapsed()
        }
    });
    until_a_lock_waits(&mut f.conn).await;
    let waiting_while_held = !deletion.is_finished();
    tokio::time::sleep(HELD).await;
    let still_waiting = !deletion.is_finished();
    release_tx.send(()).expect("connection 1 still open");
    holder.await.expect("holder").expect("connection 1 commits");
    let waited = deletion.await.expect("deletion");
    let later = db
        .run(Scope::Account(account), async |tx| {
            admin.ensure_active(tx, account).await
        })
        .await;

    // Assert
    assert!(
        waiting_while_held && still_waiting,
        "begin_deletion did not wait for the guard holder"
    );
    assert!(waited >= HELD && waited < WAIT_BOUND, "waited {waited:?}");
    assert!(
        matches!(
            later,
            Err(RunError::Closure(AccountGuardError::AccountNotActive))
        ),
        "{later:?}"
    );
    assert_eq!(
        support::accounts::state_of(&mut f.conn, account).await,
        "deleting"
    );
    drop((db, admin));
    f.finish().await;
}

#[tokio::test]
async fn guards_of_one_account_do_not_wait_for_each_other() {
    // Arrange
    let mut f = fixture().await;
    let account = f.account("user", "active").await;
    let (db, admin) = (f.harness.api.clone(), Arc::clone(&f.admin));
    let (guarded_tx, guarded_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let holder = tokio::spawn({
        let (db, admin) = (db.clone(), Arc::clone(&admin));
        async move {
            db.run(Scope::Account(account), async |tx| {
                admin.ensure_active(tx, account).await?;
                guarded_tx.send(()).expect("the test is waiting");
                release_rx.await.expect("the test releases the guard");
                Ok::<_, AccountGuardError>(())
            })
            .await
        }
    });
    guarded_rx.await.expect("connection 1 passed the guard");

    // Act
    let second = db
        .run(Scope::Account(account), async |tx| {
            admin.ensure_active(tx, account).await
        })
        .await;
    let waiting = waiting_locks(&mut f.conn).await;
    release_tx.send(()).expect("connection 1 still open");
    holder.await.expect("holder").expect("connection 1 commits");

    // Assert
    assert!(second.is_ok(), "{second:?}");
    assert_eq!(waiting, 0);
    drop((db, admin));
    f.finish().await;
}

#[tokio::test]
async fn the_guard_refuses_a_disabled_a_deleting_and_an_unknown_account() {
    // Arrange
    let mut f = fixture().await;
    let disabled = f.account("user", "disabled").await;
    let deleting = f.account("user", "deleting").await;
    let active = f.account("user", "active").await;
    let unknown = Uuid::new_v4();

    // Act
    let mut results = Vec::new();
    for account in [active, disabled, deleting, unknown] {
        let result = f
            .harness
            .api
            .run(Scope::Account(account), async |tx| {
                f.admin.ensure_active(tx, account).await
            })
            .await;
        results.push(result.is_ok());
    }

    // Assert
    assert_eq!(results, [true, false, false, false]);
    f.finish().await;
}
