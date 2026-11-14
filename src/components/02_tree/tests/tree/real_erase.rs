//! AZ-3720 AC-5 (IT-02-23 with the real identity operations, NFT-RES-30): an account created as 01 creates one
//! (its row and the real `initialize` of 07a, 05 and 02, so the root is the tree hook's), seeded with 1,000 nodes
//! and 600 revisions and moved to `deleting` by the real 01 `begin_deletion`, is erased by the tree's
//! `erase_batch` loop with the real 04 queue and sweeper: every batch keeps the IT-02-23 invariants and
//! `remaining` reaches 0.

use std::sync::Arc;

use contracts::{AccountAdmin, AccountLifecycleHook};
use sc_db::Scope;
use sunnycloud_blobstore::FileEpochMarker;
use sunnycloud_identity::IdentityAccounts;
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use sunnycloud_sync_feed::PgFeedLifecycle;

use crate::lifecycle_erase::{LIMIT, MAX_BATCHES, probe};
use crate::lifecycle_support::{Erase, NODES, REVISIONS};
use crate::mutation_support::DEFAULT_QUOTA;
use crate::real_account_repo::create_account;

#[tokio::test]
async fn an_account_of_the_real_lifecycle_is_erased_batch_by_batch_to_zero() {
    // Arrange
    let mut erase = Erase::new().await;
    let api = erase.tree.fixture.api.clone();
    let identity = IdentityAccounts::new(api.clone(), erase.clock.clone());
    let hooks: Vec<Arc<dyn AccountLifecycleHook>> = vec![
        Arc::new(PgQuotaAdmin::new(
            api.clone(),
            QuotaConfig {
                default_quota_bytes: DEFAULT_QUOTA,
            },
        )),
        Arc::new(PgFeedLifecycle::new(
            api.clone(),
            erase.clock.clone(),
            Arc::new(FileEpochMarker::new(&erase.config).expect("epoch marker")),
        )),
        erase.hook.clone(),
    ];
    let (account, root) = create_account(&erase.tree.fixture, &hooks).await;
    erase.tree.account = account;
    erase.tree.root = root;
    let seeded = erase.seed().await;
    api.run(Scope::Account(account), async |tx| {
        identity.begin_deletion(tx, account).await
    })
    .await
    .expect("begin deletion");
    let start = probe(&erase).await;

    // Act
    let mut reported = Vec::new();
    let mut probes = Vec::new();
    for _ in 0..MAX_BATCHES {
        reported.push(erase.erase(LIMIT).await);
        let after = probe(&erase).await;
        let done = after.nodes + after.revisions == 0;
        probes.push(after);
        if done {
            break;
        }
    }
    let removed = erase.sweep().await;
    let last = erase.erase(LIMIT).await;

    // Assert
    assert_eq!(
        (start.nodes, start.revisions, start.queued),
        (NODES, REVISIONS, 0)
    );
    assert!(probes.iter().all(|p| p.queued_survivors == 0));
    assert!(probes.iter().all(|p| p.unqueued_orphans == 0));
    for (remaining, after) in reported.iter().zip(&probes) {
        assert_eq!(
            *remaining as i64,
            after.nodes + after.revisions + after.queued
        );
        assert!(
            after.root || after.nodes == 0,
            "root deleted before its tree"
        );
    }
    assert!(reported.windows(2).all(|pair| pair[1] <= pair[0]));
    assert_eq!(reported.last(), Some(&(REVISIONS as u64)));
    assert_eq!(removed, REVISIONS as u64);
    assert_eq!(last, 0);
    assert!(seeded.blobs.iter().all(|&id| !erase.has_file(id)));
    assert_eq!(
        erase
            .tree
            .count(&format!(
                "SELECT count(*) FROM accounts WHERE id = '{account}' AND state = 'deleting'"
            ))
            .await,
        1
    );
    drop((identity, hooks, api));
    erase.finish().await;
}
