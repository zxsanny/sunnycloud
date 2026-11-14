//! AZ-3719 AC-2 (ST-02-2, AC-59): with the runtime role `sunnycloud` scoped to account B, raw selects on `nodes`
//! and `revisions` return no row of A, disabling row-level security on either table is denied, and the cascade
//! janitor (system scope) still sees and works on every account.

use std::sync::Arc;

use contracts::{CycleStats, Janitor, TreeMaintenance};
use sc_db::Scope;
use sqlx::Connection;
use sunnycloud_tree::TreeJanitor;
use uuid::Uuid;

use crate::maintenance_support::{Maint, config};
use crate::support::{API, START, refusal};

const PERMISSION_DENIED: &str = "42501";

const ROWS_OF_OTHERS: &str = "SELECT (SELECT count(*) FROM nodes WHERE account_id <> $1), \
    (SELECT count(*) FROM revisions WHERE account_id <> $1), \
    (SELECT count(*) FROM nodes), (SELECT count(*) FROM revisions)";

/// A deleted folder of `files` files with revisions in `account`.
async fn deleted_folder(maint: &Maint, account: Uuid, root: Uuid, files: usize) {
    let folder = maint
        .children(account, root, "folder", "F", 1)
        .await
        .remove(0);
    maint.files_with_blobs(account, folder, files).await;
    maint.delete_each(account, &[folder]).await;
}

#[tokio::test]
async fn scoped_to_b_sees_no_row_of_a_cannot_disable_security_and_the_janitor_sees_all() {
    // Arrange
    let maint = Maint::new(config(100, 100)).await;
    let (tree, a) = (maint.tree(), maint.tree().account);
    let (b, b_root) = tree.add_account().await;
    deleted_folder(&maint, a, tree.root, 20).await;
    deleted_folder(&maint, b, b_root, 10).await;
    let mut runtime = tree.fixture.db.conn_as(API).await;
    let maintenance: Arc<dyn TreeMaintenance> = maint.maintenance.clone();
    let cascade = TreeJanitor::cascade(maintenance, &maint.config, maint.erase.clock.clone());

    // Act
    let seen_by_b: (i64, i64, i64, i64) = tree
        .fixture
        .api
        .run(Scope::Account(b), async |tx| {
            sqlx::query_as(ROWS_OF_OTHERS)
                .bind(b)
                .fetch_one(tx.conn())
                .await
        })
        .await
        .expect("scoped select");
    let mut denied = Vec::new();
    for statement in [
        "ALTER TABLE nodes DISABLE ROW LEVEL SECURITY",
        "ALTER TABLE revisions DISABLE ROW LEVEL SECURITY",
    ] {
        denied.push(refusal(sqlx::raw_sql(statement).execute(&mut runtime).await).0);
    }
    let cycle = cascade.run_cycle(START).await.expect("cascade cycle");

    // Assert
    assert_eq!(
        seen_by_b,
        (0, 0, 2 + 10, 10),
        "B's root, folder and files only"
    );
    assert_eq!(denied, [PERMISSION_DENIED; 2]);
    assert_eq!(cycle, CycleStats { processed: 30 }, "files of A and of B");
    let marked = tree
        .count("SELECT count(DISTINCT account_id) FROM nodes WHERE deleted_at IS NOT NULL AND kind = 'file'")
        .await;
    assert_eq!(marked, 2);
    runtime.close().await.expect("close runtime connection");
    drop(cascade);
    maint.finish().await;
}
