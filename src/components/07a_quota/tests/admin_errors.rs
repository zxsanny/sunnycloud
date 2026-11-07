//! A database failure behind an admin call or the lifecycle hook is the unexpected tier, with the database
//! failure as its source and no SQL or constraint text in the message.

mod support;

use contracts::{AccountLifecycleHook, LifecycleError, QuotaAdminError};
use sc_db::{DbError, RunError, Scope};
use support::{GIB, Harness, admin};
use uuid::Uuid;

const FORBIDDEN_TEXT: [&str; 4] = ["quota", "SQLSTATE", "relation", "SELECT"];

fn assert_no_database_text(message: &str) {
    for word in FORBIDDEN_TEXT {
        assert!(!message.contains(word), "{word} in {message:?}");
    }
}

#[tokio::test]
async fn a_failing_table_makes_every_call_unexpected_without_database_text() {
    // Arrange
    let harness = Harness::new().await;
    let quota = admin(&harness.api, GIB);
    let id = Uuid::new_v4();
    harness.db.admin_exec("DROP TABLE quota").await;

    // Act
    let view = quota.view(id).await;
    let views = quota.views(&[id]).await;
    let set_limit = quota.set_limit(id, Some(1)).await;
    let initialize = harness
        .api
        .run(Scope::Account(id), async |tx| {
            quota.initialize(tx, id).await
        })
        .await
        .map_err(|error| match error {
            RunError::Closure(error) => error,
            RunError::Db(error) => panic!("the transaction itself failed: {error:?}"),
        });
    let erase = quota.erase_batch(id, 50).await;

    // Assert
    for result in [view.map(drop), views.map(drop), set_limit] {
        let Err(QuotaAdminError::Unexpected(source)) = result else {
            panic!("expected the unexpected tier, got {result:?}");
        };
        assert!(source.downcast_ref::<DbError>().is_some());
        assert_no_database_text(&QuotaAdminError::Unexpected(source).to_string());
    }
    for result in [initialize, erase.map(drop)] {
        let Err(LifecycleError::Unexpected(source)) = result else {
            panic!("expected the unexpected tier, got {result:?}");
        };
        assert!(source.downcast_ref::<DbError>().is_some());
        assert_no_database_text(&LifecycleError::Unexpected(source).to_string());
    }
    harness.finish().await;
}
