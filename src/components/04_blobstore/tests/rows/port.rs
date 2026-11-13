//! AZ-4264 AC-1: 04's registry is the `contracts::BlobRegistry` port, and every method is filtered by the account
//! it names: inside account A's transaction, A's blob named with account B behaves like an absent row.

use contracts::{BlobRegistry, ReinstateOutcome};
use sc_db::{RunError, Scope};

use crate::support::{Rig, account, committed_blob};

#[tokio::test]
async fn through_the_port_a_blob_named_with_another_account_is_an_absent_row() {
    // Arrange
    let rig = Rig::new().await;
    let (a, b) = (account(&rig).await, account(&rig).await);
    let blob = committed_blob(&rig, a).await;
    rig.put_file(blob);
    let port: &dyn BlobRegistry = &rig.registry;

    // Act
    let commit = rig
        .api
        .run(Scope::Account(a), async |tx| {
            port.mark_committed_in_tx(tx, b, blob, 99).await
        })
        .await;
    let reinstate = rig
        .api
        .run(Scope::Account(a), async |tx| {
            let reinstate = port.reinstate_in_tx(tx, b, blob).await?;
            port.remove_row_in_tx(tx, b, blob).await?;
            Ok::<_, contracts::BlobRegistryError>(reinstate)
        })
        .await
        .expect("foreign account calls");
    let after_foreign = rig.row(blob).await;
    rig.api
        .run(Scope::Account(a), async |tx| {
            port.remove_row_in_tx(tx, a, blob).await
        })
        .await
        .expect("remove as owner");

    // Assert
    assert!(matches!(commit, Err(RunError::Closure(_))), "{commit:?}");
    assert_eq!(reinstate, ReinstateOutcome::BeyondRetention);
    assert_eq!(after_foreign, Some(("committed".to_owned(), Some(10))));
    assert_eq!(rig.row(blob).await, None);
    rig.finish().await;
}
