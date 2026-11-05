//! AC-4 (rollback on error and panic) and AC-5 (savepoints).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Barrier;
use uuid::Uuid;

use super::support::{TestDb, insert_note};
use crate::{Db, DbError, RunError, Scope};

/// Every one of the pool's connections is usable at the same time.
async fn assert_pool_full(api: &Db, size: usize) {
    let barrier = Arc::new(Barrier::new(size));
    let mut tasks = Vec::new();
    for _ in 0..size {
        let (api, barrier) = (api.clone(), barrier.clone());
        tasks.push(tokio::spawn(async move {
            api.run_system("test_pool", async |_tx| {
                barrier.wait().await;
                Ok::<_, DbError>(())
            })
            .await
        }));
    }
    for task in tasks {
        let finished = tokio::time::timeout(Duration::from_secs(20), task)
            .await
            .expect("pool lost a connection: not all runs got one at the same time");
        finished.expect("task").expect("run");
    }
}

#[tokio::test]
async fn closure_error_rolls_back_and_is_returned() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let account = Uuid::new_v4();

    let result = api
        .run(Scope::Account(account), async |tx| {
            insert_note(tx, account, "gone").await.expect("insert");
            Err::<(), _>("business failure")
        })
        .await;

    assert!(matches!(result, Err(RunError::Closure("business failure"))));
    assert_eq!(db.count_notes().await, 0);
    assert_pool_full(&api, 5).await;
    db.drop_db().await;
}

#[tokio::test]
async fn closure_panic_rolls_back_and_the_pool_keeps_its_size() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let account = Uuid::new_v4();
    let panicking = api.clone();

    let outcome = tokio::spawn(async move {
        panicking
            .run(Scope::Account(account), async |tx| {
                insert_note(tx, account, "gone").await?;
                std::hint::black_box(Err::<(), DbError>(DbError::CheckViolation))
                    .expect("closure panics after inserting");
                Ok::<(), DbError>(())
            })
            .await
    })
    .await;

    assert!(
        outcome
            .expect_err("the panic must reach the caller")
            .is_panic()
    );
    assert_eq!(db.count_notes().await, 0);
    assert_pool_full(&api, 5).await;
    api.run(Scope::Account(account), async |tx| {
        insert_note(tx, account, "kept").await
    })
    .await
    .expect("the pool serves the next run");
    assert_eq!(db.count_notes().await, 1);
    db.drop_db().await;
}

#[tokio::test]
async fn failed_savepoint_leaves_the_outer_transaction_usable() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let account = Uuid::new_v4();

    let inner = api
        .run(Scope::Account(account), async |tx| {
            insert_note(tx, account, "first").await?;
            let failed = tx
                .savepoint(async |tx| insert_note(tx, account, "first").await)
                .await;
            insert_note(tx, account, "second").await?;
            Ok::<_, DbError>(failed)
        })
        .await
        .expect("the outer transaction commits");

    assert_eq!(
        inner,
        Err(DbError::UniqueViolation {
            constraint: Some("notes_account_body_key".into())
        })
    );
    assert_eq!(db.count_notes().await, 2);
    db.drop_db().await;
}

#[tokio::test]
async fn successful_savepoint_keeps_its_work_and_nested_savepoints_roll_back_alone() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let account = Uuid::new_v4();

    api.run(Scope::Account(account), async |tx| {
        tx.savepoint(async |tx| {
            insert_note(tx, account, "outer").await?;
            let nested = tx
                .savepoint(async |tx| {
                    insert_note(tx, account, "inner").await?;
                    Err::<(), _>(DbError::CheckViolation)
                })
                .await;
            assert_eq!(nested, Err(DbError::CheckViolation));
            Ok::<_, DbError>(())
        })
        .await
    })
    .await
    .expect("run");

    assert_eq!(db.count_notes().await, 1);
    db.drop_db().await;
}
