//! AC-9: the account advisory lock serialises one account and no other.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use uuid::Uuid;

use super::support::TestDb;
use crate::{Db, DbError, Scope};

type Log = Arc<Mutex<Vec<&'static str>>>;

fn note(log: &Log, event: &'static str) {
    log.lock().expect("log").push(event);
}

/// Holds the lock of `first` for 700 ms while a second run locks `second`; returns the event order.
async fn contend(api: &Db, first: Uuid, second: Uuid) -> Vec<&'static str> {
    let log: Log = Arc::default();
    let (locked, wait_locked) = oneshot::channel();
    let holder = {
        let (api, log) = (api.clone(), log.clone());
        tokio::spawn(async move {
            api.run(Scope::Account(first), async |tx| {
                tx.advisory_lock_account(first).await?;
                note(&log, "first locked");
                locked.send(()).expect("signal");
                tokio::time::sleep(Duration::from_millis(700)).await;
                note(&log, "first done");
                Ok::<_, DbError>(())
            })
            .await
            .expect("first run");
        })
    };
    wait_locked.await.expect("first lock taken");
    let waiter = {
        let (api, log) = (api.clone(), log.clone());
        tokio::spawn(async move {
            api.run(Scope::Account(second), async |tx| {
                tx.advisory_lock_account(second).await?;
                note(&log, "second locked");
                Ok::<_, DbError>(())
            })
            .await
            .expect("second run");
        })
    };
    holder.await.expect("holder");
    waiter.await.expect("waiter");
    log.lock().expect("log").clone()
}

#[tokio::test]
async fn same_account_waits_for_the_first_commit() {
    let db = TestDb::empty().await;
    let api = db.api().await;
    let account = Uuid::new_v4();

    let events = contend(&api, account, account).await;

    assert_eq!(events, ["first locked", "first done", "second locked"]);
    db.drop_db().await;
}

#[tokio::test]
async fn different_accounts_do_not_wait() {
    let db = TestDb::empty().await;
    let api = db.api().await;

    let events = contend(&api, Uuid::new_v4(), Uuid::new_v4()).await;

    assert_eq!(events, ["first locked", "second locked", "first done"]);
    db.drop_db().await;
}
