//! AZ-3845 AC-1 (IT-08-15 erase, AC-27) and AC-4 (IT-08-15 cleanup): the account erase hook and the blob owner
//! cleanup over the real 04 `GcQueue` and `GcSweeper`.

use std::sync::Arc;

use contracts::{AccountLifecycleHook, BlobOwnerCleanup, Due, GcQueue as _, GcReason};
use media_api::MediaLifecycle;
use sc_clock::CancellationToken;
use sc_db::{DbError, Scope};
use sqlx::Connection;
use sunnycloud_blobstore::GcSweeper;
use uuid::Uuid;

use crate::accept_support::Accept;
use crate::blob_support::WEBP;
use crate::maint_support::{gc_rows, rows_of, seed_derived, seed_jobs};
use crate::support::{DerivedRow, derived};

const LIMIT: u32 = 50;

fn lifecycle(a: &Accept) -> MediaLifecycle {
    MediaLifecycle::new(a.q.fixture.api.clone(), a.gc.clone())
}

#[tokio::test]
async fn erase_batch_drains_jobs_then_derived_rows_in_calls_of_at_most_the_limit() {
    // Arrange
    let a = Accept::new().await;
    let (other, other_revision) = a.q.second_account().await;
    let mut conn = a.q.fixture.owner_conn().await;
    seed_jobs(&mut conn, a.q.account, a.q.revision, 120).await;
    let blobs = seed_derived(&mut conn, a.q.account, None, 200).await;
    seed_jobs(&mut conn, other, other_revision, 8).await;
    seed_derived(&mut conn, other, None, 6).await;
    conn.close().await.expect("close owner connection");
    let hook = lifecycle(&a);

    // Act
    let mut calls = Vec::new();
    loop {
        let before = rows_of(&a.q, a.q.account).await;
        let progress = hook
            .erase_batch(a.q.account, LIMIT)
            .await
            .expect("erase batch");
        let after = rows_of(&a.q, a.q.account).await;
        calls.push((before, after, progress.remaining));
        if progress.remaining == 0 {
            break;
        }
        assert!(calls.len() < 20, "the erase does not drain: {calls:?}");
    }
    let again = hook
        .erase_batch(a.q.account, LIMIT)
        .await
        .expect("call after 0");
    let unknown = hook
        .erase_batch(Uuid::new_v4(), LIMIT)
        .await
        .expect("unknown account");

    // Assert
    assert_eq!(calls.len(), 7, "{calls:?}");
    for ((jobs_before, derived_before), (jobs_after, derived_after), remaining) in &calls {
        let deleted = (jobs_before - jobs_after) + (derived_before - derived_after);
        assert!(deleted <= i64::from(LIMIT), "{calls:?}");
        assert_eq!(
            *remaining,
            u64::try_from(jobs_after + derived_after).expect("count")
        );
        if *jobs_after > 0 {
            assert_eq!(*derived_after, 200, "jobs go first: {calls:?}");
        }
    }
    assert_eq!((again.remaining, unknown.remaining), (0, 0));
    let queued = gc_rows(&a.q, a.q.account).await;
    let expected: Vec<(Uuid, String, bool)> = blobs
        .iter()
        .map(|blob| (*blob, "account_deleted".to_owned(), true))
        .collect();
    assert_eq!(queued, expected);
    assert_eq!(rows_of(&a.q, other).await, (8, 6));
    assert!(gc_rows(&a.q, other).await.is_empty());
    drop(hook);
    a.finish().await;
}

/// A real derived blob (file and committed row) owned by a derived row of the account.
async fn owned_blob(a: &Accept, n: u8) -> Uuid {
    let blob = a.blobs.put(&a.q.fixture, a.q.account, &WEBP).await;
    let mut conn = a.q.fixture.owner_conn().await;
    derived(
        &mut conn,
        &DerivedRow {
            account: a.q.account,
            content_hash: &[n; 32],
            kind: "thumb",
            params_version: 1,
            blob,
        },
    )
    .await
    .expect("derived row");
    conn.close().await.expect("close owner connection");
    blob
}

async fn derived_of(a: &Accept, blob: Uuid) -> i64 {
    let mut conn = a.q.fixture.owner_conn().await;
    let count = sqlx::query_scalar("SELECT count(*) FROM derived WHERE blob_id = $1")
        .bind(blob)
        .fetch_one(&mut conn)
        .await
        .expect("derived count");
    conn.close().await.expect("close owner connection");
    count
}

#[tokio::test]
async fn the_gc_sweeper_removes_the_owning_derived_row_with_the_blob_in_its_transaction() {
    // Arrange
    let a = Accept::new().await;
    let blob = owned_blob(&a, 7).await;
    a.q.fixture
        .api
        .run(Scope::Account(a.q.account), async |tx| {
            a.gc.schedule_in_tx(tx, blob, GcReason::DerivedRemoved, Due::Now)
                .await
        })
        .await
        .expect("queue the blob");
    let sweeper = GcSweeper::new(
        &a.blobs.config,
        a.q.fixture.api.clone(),
        a.q.clock.clone(),
        a.blobs.registry.clone(),
        vec![Arc::new(lifecycle(&a))],
        CancellationToken::new(),
    )
    .expect("sweeper");

    // Act
    let cycle = sweeper.run_cycle(a.q.clock_now()).await.expect("gc cycle");

    // Assert
    assert_eq!(cycle.removed, 1);
    assert_eq!(derived_of(&a, blob).await, 0);
    assert!(!a.blobs.files().contains(&blob));
    drop(sweeper);
    a.finish().await;
}

#[tokio::test]
async fn a_rolled_back_cleanup_keeps_the_row_and_a_blob_without_a_row_is_success() {
    // Arrange
    let a = Accept::new().await;
    let blob = owned_blob(&a, 8).await;
    let kept = owned_blob(&a, 9).await;
    let cleanup = lifecycle(&a);

    // Act
    let rolled_back =
        a.q.fixture
            .api
            .run(Scope::Account(a.q.account), async |tx| {
                cleanup
                    .remove_owner_rows(tx, blob)
                    .await
                    .expect("cleanup in the transaction");
                Err::<(), DbError>(DbError::Other("caller rolls back".to_owned()))
            })
            .await;
    let no_row =
        a.q.fixture
            .api
            .run(Scope::Account(a.q.account), async |tx| {
                cleanup.remove_owner_rows(tx, Uuid::new_v4()).await
            })
            .await;

    // Assert
    assert!(rolled_back.is_err());
    assert!(no_row.is_ok(), "{no_row:?}");
    assert_eq!(
        (derived_of(&a, blob).await, derived_of(&a, kept).await),
        (1, 1)
    );
    drop(cleanup);
    a.finish().await;
}
