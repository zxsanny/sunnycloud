//! A vault folder for the 03 rig (AZ-3927). Its membership is made by the real 02 `VaultRoots::claim_root` of a
//! `PgTreeService` on the rig's database, inside the transaction that inserts the `vaults` row, as 20's
//! `create_vault` does (03 has no 20 dev-dependency); nothing writes `nodes.vault_id` by hand.

use std::sync::Arc;

use contracts::VaultRoots;
use sc_clock::Clock;
use sc_db::Scope;
use sunnycloud_blobstore::GcQueue;
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::PgTreeService;
use uuid::Uuid;

use crate::rig_config::blob_config;
use crate::start_support::{GIB, Rig, close, node};

impl Rig {
    /// A new folder under the root, made the root of a vault of the rig's account; returns the folder.
    pub async fn vault_folder(&self, name: &str) -> Uuid {
        let mut owner = self.owner().await;
        let folder = node(&mut owner, self.account, Some(self.root), "folder", name).await;
        close(owner).await;
        let clock: Arc<dyn Clock> = self.clock.clone();
        let key_file = self.volume.with_file_name("master.key");
        let gc = GcQueue::new(
            &blob_config(&self.volume, &key_file, 0),
            self.fixture.api.clone(),
            clock.clone(),
        )
        .expect("gc queue");
        let tree = PgTreeService::new(
            self.fixture.api.clone(),
            Arc::new(StateGuard::default()),
            Arc::new(PgQuotaLedger::new(QuotaConfig {
                default_quota_bytes: 10 * GIB,
            })),
            Arc::new(gc),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            clock,
        );
        let (account, vault) = (self.account, Uuid::new_v4());
        self.fixture
            .api
            .run(Scope::Account(account), async |tx| {
                sqlx::query(
                    "INSERT INTO vaults (id, account_id, root_node_id, header, created_at, updated_at) \
                     VALUES ($1, $2, $3, '\\x01'::bytea, 'epoch'::timestamptz, 'epoch'::timestamptz)",
                )
                .bind(vault)
                .bind(account)
                .bind(folder)
                .execute(tx.conn())
                .await
                .map_err(|error| error.to_string())?;
                tree.claim_root(tx, account, folder, vault)
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .expect("make the folder a vault root");
        folder
    }
}
