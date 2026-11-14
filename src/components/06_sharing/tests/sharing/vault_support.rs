//! A vault for the content tests' owner (AZ-3927). Its root's membership is made by the real 02
//! `VaultRoots::claim_root` of the owner's `PgTreeService`, inside the transaction that inserts the `vaults` row, as
//! 20's `create_vault` does (06 has no 20 dev-dependency); files under it copy the membership through 02's publish.

use contracts::VaultRoots;
use sc_db::Scope;
use uuid::Uuid;

use crate::content_support::Owner;
use crate::harness::Harness;

impl Owner {
    /// Makes the empty folder `folder` the root of the owner's vault; returns the vault id.
    pub async fn make_vault(&self, harness: &Harness, folder: Uuid) -> Uuid {
        let (account, vault) = (self.account, Uuid::new_v4());
        harness
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
                self.tree
                    .claim_root(tx, account, folder, vault)
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .expect("make the folder a vault root");
        vault
    }
}
