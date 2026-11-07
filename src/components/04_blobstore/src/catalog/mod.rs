//! `BlobCatalog`: the account-scoped lookup of a blob's key material.

mod catalog_repo;

use contracts::{BlobCatalogError, BlobRef, async_trait};
use sc_db::Db;
use uuid::Uuid;

use crate::error::{BlobIds, run_fault};

pub struct BlobCatalog {
    db: Db,
}

impl BlobCatalog {
    pub fn new(db: Db) -> Self {
        BlobCatalog { db }
    }
}

#[async_trait]
impl contracts::BlobCatalog for BlobCatalog {
    /// A row still `uploading` has no size and no readable content: it is `None` like a foreign id.
    async fn get_ref(
        &self,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<Option<BlobRef>, BlobCatalogError> {
        let ids = BlobIds {
            blob_id: Some(blob_id),
            account_id: Some(account),
        };
        let row = catalog_repo::get_ref(&self.db, account, blob_id)
            .await
            .map_err(|error| BlobCatalogError::Unexpected(run_fault(error, ids)))?;
        Ok(row.map(|(size, wrapped_key, master_key_id)| BlobRef {
            blob_id,
            size,
            wrapped_key,
            master_key_id,
        }))
    }
}
