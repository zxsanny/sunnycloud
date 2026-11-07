//! AC-1: rows follow the caller's transaction.

use std::time::Duration;

use contracts::{RegisterState, WrittenBlob, WrittenOrPlanned};
use sc_db::{RunError, Scope};
use sunnycloud_blobstore::{BlobRegistry, BlobstoreConfig};
use uuid::Uuid;

use crate::support::{Rig, START, Stop, account, config, plan};

#[tokio::test]
async fn register_then_mark_committed_commits_a_committed_row() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let id = Uuid::new_v4();

    // Act
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.registry
                .register_in_tx(
                    tx,
                    a,
                    WrittenOrPlanned::Planned(plan(id)),
                    RegisterState::Uploading,
                )
                .await?;
            rig.registry.mark_committed_in_tx(tx, id, 42).await?;
            Ok::<_, Stop>(())
        })
        .await
        .expect("commit");

    // Assert
    assert_eq!(rig.row(id).await, Some(("committed".to_owned(), Some(42))));
    rig.finish().await;
}

#[tokio::test]
async fn caller_rollback_leaves_no_row() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let id = Uuid::new_v4();

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry
                .register_in_tx(
                    tx,
                    a,
                    WrittenOrPlanned::Planned(plan(id)),
                    RegisterState::Uploading,
                )
                .await?;
            rig.registry.mark_committed_in_tx(tx, id, 42).await?;
            Err::<(), _>(Stop::Rollback)
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(Stop::Rollback))));
    assert_eq!(rig.row(id).await, None);
    rig.finish().await;
}

#[tokio::test]
async fn written_blob_registers_committed_with_its_size_and_the_clock_time() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let id = Uuid::new_v4();
    rig.clock.tick(Duration::from_secs(90));
    let written = WrittenBlob {
        plan: plan(id),
        size: 7,
    };

    // Act
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.registry
                .register_in_tx(
                    tx,
                    a,
                    WrittenOrPlanned::Written(written),
                    RegisterState::Committed,
                )
                .await
        })
        .await
        .expect("register");

    // Assert
    assert_eq!(rig.row(id).await, Some(("committed".to_owned(), Some(7))));
    assert_eq!(
        rig.created_micros(id).await,
        START.as_unix_micros() + 90_000_000
    );
    rig.finish().await;
}

#[tokio::test]
async fn planned_blob_cannot_register_committed_because_it_has_no_size() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let id = Uuid::new_v4();

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry
                .register_in_tx(
                    tx,
                    a,
                    WrittenOrPlanned::Planned(plan(id)),
                    RegisterState::Committed,
                )
                .await
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(_))));
    assert_eq!(rig.row(id).await, None);
    rig.finish().await;
}

#[tokio::test]
async fn mark_committed_fails_for_an_absent_or_already_committed_row() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let committed = crate::support::committed_blob(&rig, a).await;

    // Act
    let absent = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry
                .mark_committed_in_tx(tx, Uuid::new_v4(), 1)
                .await
        })
        .await;
    let again = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry.mark_committed_in_tx(tx, committed, 99).await
        })
        .await;

    // Assert
    assert!(matches!(absent, Err(RunError::Closure(_))));
    assert!(matches!(again, Err(RunError::Closure(_))));
    assert_eq!(
        rig.row(committed).await,
        Some(("committed".to_owned(), Some(10)))
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_transaction_cannot_register_a_row_for_another_account() {
    // Arrange
    let rig = Rig::new().await;
    let (a, b) = (account(&rig).await, account(&rig).await);
    let id = Uuid::new_v4();

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry
                .register_in_tx(
                    tx,
                    b,
                    WrittenOrPlanned::Planned(plan(id)),
                    RegisterState::Uploading,
                )
                .await
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(_))));
    assert_eq!(rig.row(id).await, None);
    rig.finish().await;
}

#[tokio::test]
async fn registry_needs_a_blob_root() {
    // Arrange
    let mut settings: BlobstoreConfig = config(std::path::Path::new("/unused"), Duration::ZERO);
    settings.blob_root = None;

    // Act
    let result = BlobRegistry::new(&settings, sc_testdb::clock_at_start());

    // Assert
    assert!(result.is_err());
}
