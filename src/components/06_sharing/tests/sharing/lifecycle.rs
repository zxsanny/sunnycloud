//! IT-06-10 account erase hook (AZ-3702) on a real PostgreSQL 18: AC-1 (batches of 50 over 120 links report 70,
//! 20, 0 and 0 again; another account's links stay), AC-2 (links on tombstoned files answer `NotFound` before the
//! erase, every erased link after it) and AC-3 (`initialize` writes nothing). The hook runs as `sunnycloud`;
//! fixtures and probes run as the schema owner.

use contracts::{AccountLifecycleHook, ShareOpen, ShareService};
use sc_db::Scope;
use sc_ids::RawToken;
use sqlx::{Connection, PgConnection};
use sunnycloud_sharing::PgShareLifecycle;
use uuid::Uuid;

use crate::harness::{Harness, account, account_with_file, link};
use crate::owner_support::{account_with_root, tombstone};
use crate::visitor_support::{HOUR, VISITOR, named_file};

async fn links_of(conn: &mut PgConnection, account: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT row_to_json(s)::text FROM share_links s WHERE account_id = $1 ORDER BY id",
    )
    .bind(account)
    .fetch_all(conn)
    .await
    .expect("read the account's links")
}

async fn link_exists(conn: &mut PgConnection, link: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM share_links WHERE id = $1)")
        .bind(link)
        .fetch_one(conn)
        .await
        .expect("probe the link")
}

/// `count` links on one file of a new account, token hashes made of `first_seed..`; returns the account.
async fn account_with_links(conn: &mut PgConnection, count: u8, first_seed: u8) -> Uuid {
    let id = Uuid::new_v4();
    let file = account_with_file(conn, id).await;
    for seed in first_seed..first_seed + count {
        link(conn, id, file, seed).await.expect("insert link");
    }
    id
}

#[tokio::test]
async fn batches_of_50_over_120_links_report_70_20_0_and_0_again_and_leave_the_other_account() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let erased = account_with_links(&mut conn, 120, 0).await;
    let other = account_with_links(&mut conn, 3, 200).await;
    let other_before = links_of(&mut conn, other).await;
    let hook = PgShareLifecycle::new(harness.api.clone());

    // Act
    let mut reports = Vec::new();
    loop {
        let progress = hook.erase_batch(erased, 50).await.expect("erase batch");
        reports.push(progress.remaining);
        if progress.remaining == 0 {
            break;
        }
    }
    let again = hook.erase_batch(erased, 50).await.expect("erase again");
    reports.push(again.remaining);

    // Assert
    assert_eq!(reports, [70, 20, 0, 0]);
    assert!(links_of(&mut conn, erased).await.is_empty());
    assert_eq!(other_before.len(), 3);
    assert_eq!(links_of(&mut conn, other).await, other_before);
    conn.close().await.expect("close fixture connection");
    drop(hook);
    harness.finish().await;
}

#[tokio::test]
async fn tombstoned_links_are_not_found_before_the_erase_and_every_erased_link_after_it() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let shares = harness.shares();
    let mut links: Vec<(Uuid, RawToken, bool)> = Vec::new();
    for (index, tombstoned) in [false, true, false, true, false].into_iter().enumerate() {
        let name = format!("file-{index}.txt");
        let file = named_file(&mut conn, account, root, &name, 10, "text/plain").await;
        let created = shares
            .create(account, file, Some(24 * HOUR), None)
            .await
            .expect("create link");
        if tombstoned {
            tombstone(&mut conn, file).await;
        }
        links.push((created.id, created.token, tombstoned));
    }
    let hook = PgShareLifecycle::new(harness.api.clone());

    // Act
    let mut before = Vec::new();
    for (_, token, tombstoned) in &links {
        before.push((
            *tombstoned,
            shares.open(token, VISITOR).await.expect("open"),
        ));
    }
    let mut after_erase = Vec::new();
    let mut remaining = u64::MAX;
    while remaining > 0 {
        remaining = hook
            .erase_batch(account, 2)
            .await
            .expect("erase batch")
            .remaining;
        for (id, token, _) in &links {
            if !link_exists(&mut conn, *id).await {
                after_erase.push(shares.open(token, VISITOR).await.expect("open"));
            }
        }
    }

    // Assert
    for (tombstoned, answer) in &before {
        if *tombstoned {
            assert_eq!(*answer, ShareOpen::NotFound);
        } else {
            assert!(matches!(answer, ShareOpen::Available { .. }), "{answer:?}");
        }
    }
    // Batches of 2 over 5 links: 2 erased after the first, 4 after the second, 5 after the third.
    assert_eq!(after_erase.len(), 2 + 4 + 5);
    assert!(
        after_erase
            .iter()
            .all(|answer| *answer == ShareOpen::NotFound)
    );
    conn.close().await.expect("close fixture connection");
    drop((hook, shares));
    harness.finish().await;
}

#[tokio::test]
async fn initialize_writes_nothing_in_the_callers_transaction() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let created = Uuid::new_v4();
    account(&mut conn, created).await;
    let hook = PgShareLifecycle::new(harness.api.clone());

    // Act
    let write_id: Option<String> = harness
        .api
        .run(Scope::Account(created), async |tx| {
            hook.initialize(tx, created)
                .await
                .map_err(|error| error.to_string())?;
            // A transaction gets a transaction id only when it writes.
            sqlx::query_scalar("SELECT txid_current_if_assigned()::text")
                .fetch_one(tx.conn())
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .expect("initialize in the caller's transaction");

    // Assert
    assert_eq!(write_id, None);
    assert!(links_of(&mut conn, created).await.is_empty());
    conn.close().await.expect("close fixture connection");
    drop(hook);
    harness.finish().await;
}
