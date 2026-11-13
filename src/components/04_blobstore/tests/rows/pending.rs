//! AC-5: the pending count is per account and excludes removed blobs.

use contracts::{BlobRegistry as _, Due, GcQueue, GcReason};
use sc_db::Scope;
use uuid::Uuid;

use crate::support::{Rig, account, committed_blob};

async fn queued(rig: &Rig, owner: Uuid, count: usize) -> Vec<Uuid> {
    let mut blobs = Vec::new();
    for _ in 0..count {
        let blob = committed_blob(rig, owner).await;
        rig.api
            .run(Scope::Account(owner), async |tx| {
                rig.queue
                    .schedule_in_tx(tx, blob, GcReason::Deleted, Due::AfterRetention)
                    .await
            })
            .await
            .expect("schedule");
        blobs.push(blob);
    }
    blobs
}

#[tokio::test]
async fn counts_exactly_the_queued_unremoved_blobs_of_the_account() {
    // Arrange
    let rig = Rig::new().await;
    let (a, b, empty) = (
        account(&rig).await,
        account(&rig).await,
        account(&rig).await,
    );
    let a_blobs = queued(&rig, a, 3).await;
    queued(&rig, b, 2).await;
    committed_blob(&rig, a).await;

    // Act
    let before = rig.queue.pending_for_account(a).await.expect("count");
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.registry.remove_row_in_tx(tx, a, a_blobs[0]).await
        })
        .await
        .expect("remove");
    let after = rig.queue.pending_for_account(a).await.expect("count");
    let other = rig.queue.pending_for_account(b).await.expect("count");
    let none = rig.queue.pending_for_account(empty).await.expect("count");

    // Assert
    assert_eq!((before, after, other, none), (3, 2, 2, 0));
    rig.finish().await;
}
