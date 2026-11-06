//! AC-3: 100 concurrent reservations of 50 GiB against a 1 TiB limit, each in a transaction of its own on its own
//! pool connection, while a reader samples the row every 5 ms.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use contracts::{QuotaLedger, QuotaLedgerError};
use sc_db::{DbError, Scope};
use sqlx::Connection;
use support::{GIB, Harness, TIB, account_with_quota, ledger, quota_row};
use uuid::Uuid;

const TASKS: usize = 100;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hundred_concurrent_reservations_never_overshoot_the_limit() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(TIB), 0, 0).await;
    let db = harness.api_with_pool(32).await;
    let done = Arc::new(AtomicBool::new(false));
    let sampler = {
        let done = Arc::clone(&done);
        let mut reader = harness.fixture_conn().await;
        tokio::spawn(async move {
            let mut samples = Vec::new();
            while !done.load(Ordering::Acquire) {
                samples.push(quota_row(&mut reader, account).await.2);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            reader.close().await.expect("close sampler connection");
            samples
        })
    };

    // Act
    let tasks: Vec<_> = (0..TASKS)
        .map(|_| {
            let (db, ledger) = (db.clone(), ledger(TIB));
            tokio::spawn(async move {
                db.run(Scope::Account(account), async |tx| {
                    Ok::<_, DbError>(ledger.reserve(tx, account, 50 * GIB).await)
                })
                .await
                .expect("transaction")
            })
        })
        .collect();
    let mut granted = 0;
    let mut refused = 0;
    for task in tasks {
        match task.await.expect("task") {
            Ok(_) => granted += 1,
            Err(QuotaLedgerError::QuotaExceeded { .. }) => refused += 1,
            Err(other) => panic!("unexpected ledger error: {other:?}"),
        }
    }
    done.store(true, Ordering::Release);
    let samples = sampler.await.expect("sampler");

    // Assert
    assert_eq!((granted, refused), (20, 80));
    assert!(samples.len() > 1, "{} samples", samples.len());
    assert!(
        samples.iter().all(|reserved| *reserved <= TIB),
        "{samples:?}"
    );
    assert_eq!(
        quota_row(&mut conn, account).await,
        (Some(TIB), 0, 1000 * GIB)
    );
    conn.close().await.expect("close fixture connection");
    drop(db);
    harness.finish().await;
}
