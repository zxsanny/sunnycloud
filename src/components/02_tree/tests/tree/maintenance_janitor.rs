//! AZ-3714 AC-4 (ST-02-2): one janitor cycle drains the due work of two accounts in system scope, and a cycle
//! with nothing due writes nothing and reports 0.

use std::sync::Arc;

use contracts::{CycleStats, Janitor, TreeMaintenance};
use sunnycloud_tree::TreeJanitor;
use uuid::Uuid;

use crate::maintenance_support::{Maint, PURGE_AT, config};
use crate::statements::StatementLog;
use crate::support::START;

/// A deleted folder of 30 files in `account`; returns the folder.
async fn deleted_folder(maint: &Maint, account: Uuid, root: Uuid) -> Uuid {
    let folder = maint
        .children(account, root, "folder", "F", 1)
        .await
        .remove(0);
    maint.children(account, folder, "file", "f", 30).await;
    maint.delete_each(account, &[folder]).await;
    folder
}

fn janitors(maint: &Maint) -> (TreeJanitor, TreeJanitor) {
    let maintenance: Arc<dyn TreeMaintenance> = maint.maintenance.clone();
    let clock = maint.erase.clock.clone();
    (
        TreeJanitor::cascade(maintenance.clone(), &maint.config, clock.clone()),
        TreeJanitor::purge(maintenance, &maint.config, clock),
    )
}

#[tokio::test]
async fn one_cycle_of_each_janitor_drains_two_accounts() {
    // Arrange
    let maint = Maint::new(config(20, 10)).await;
    let (tree, first) = (maint.tree(), maint.tree().account);
    let (second, second_root) = tree.add_account().await;
    deleted_folder(&maint, first, tree.root).await;
    deleted_folder(&maint, second, second_root).await;
    let (cascade, purge) = janitors(&maint);

    // Act
    let cascaded = cascade.run_cycle(START).await.expect("cascade cycle");
    let purged = purge.run_cycle(PURGE_AT).await.expect("purge cycle");

    // Assert
    assert_eq!(cascaded, CycleStats { processed: 60 });
    assert_eq!(purged, CycleStats { processed: 62 }, "31 nodes per account");
    let left = maint
        .count("SELECT count(*) FROM nodes WHERE parent_id IS NOT NULL")
        .await;
    assert_eq!(left, 0, "only the two roots remain");
    for account in [first, second] {
        let seq = maint
            .count(&format!(
                "SELECT seq FROM account_feeds WHERE account_id = '{account}'"
            ))
            .await;
        assert_eq!(
            maint.purge_horizon(account).await,
            seq,
            "the folder's tombstone"
        );
    }
    drop((cascade, purge));
    maint.finish().await;
}

#[tokio::test]
async fn a_cycle_with_nothing_due_writes_nothing_and_reports_zero() {
    // Arrange
    let maint = Maint::new(config(20, 10)).await;
    let tree = maint.tree();
    let fresh = maint
        .children(tree.account, tree.root, "file", "young", 5)
        .await;
    maint.delete_each(tree.account, &fresh).await;
    let (cascade, purge) = janitors(&maint);
    let log = StatementLog::install();

    // Act
    let cascaded = cascade.run_cycle(START).await.expect("cascade cycle");
    let purged = purge.run_cycle(START).await.expect("purge cycle");
    let statements = log.take();

    // Assert
    assert_eq!((cascaded.processed, purged.processed), (0, 0));
    let writes: Vec<&String> = statements
        .iter()
        .filter(|s| {
            ["UPDATE", "DELETE", "INSERT"]
                .iter()
                .any(|verb| s.trim_start().starts_with(verb))
        })
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
    assert_eq!(
        statements.iter().filter(|s| s.trim() == "COMMIT").count(),
        2
    );
    drop((cascade, purge));
    maint.finish().await;
}
