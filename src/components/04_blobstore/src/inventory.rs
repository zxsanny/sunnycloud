//! The missing-blob inventory: committed rows whose file is absent from the volume (AC-73).
//!
//! A missing file is a finding returned to the caller, never an error. The walk goes through `blobs` in key
//! order, one short transaction per page, and checks the files of the page one after the other (each check is one
//! job on the blocking pool), so the volume sees one lookup at a time and no transaction is open during a lookup.
//! `uploading` and `gc_pending` rows are not walked: an upload has no complete file yet and a collected blob is
//! meant to go.

use std::io::{self, ErrorKind};
use std::path::PathBuf;

use contracts::{BlobError, InventoryError, async_trait};
use sc_db::Db;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_io, io_fault, run_fault};
use crate::path::blob_path;
use crate::scrub::scrub_repo;

/// What one page of the walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPage {
    /// Ids on the page whose file is absent.
    pub missing: Vec<Uuid>,
    /// Where the next page starts; `None` when this page reached the last committed row.
    pub next: Option<Uuid>,
}

pub struct BlobInventory {
    db: Db,
    root: PathBuf,
    /// The last id of the previous `find_missing` page. Held across a call, so two callers take turns.
    cursor: Mutex<Option<Uuid>>,
}

impl BlobInventory {
    /// `config.blob_root` is required.
    pub fn new(config: &BlobstoreConfig, db: Db) -> Result<Self, BlobError> {
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| {
                blob_error_from_io(io::Error::from(ErrorKind::InvalidInput), BlobIds::default())
            })?;
        Ok(BlobInventory {
            db,
            root: root.into(),
            cursor: Mutex::new(None),
        })
    }

    /// Checks the next `batch` (at least one) committed rows after `after`, in key order. Stateless: the caller
    /// holds the position, starts with `None` and stops when `next` is `None`.
    pub async fn scan_page(
        &self,
        after: Option<Uuid>,
        batch: u32,
    ) -> Result<MissingPage, InventoryError> {
        let limit = batch.max(1);
        let ids = scrub_repo::committed_page(&self.db, after, i64::from(limit))
            .await
            .map_err(|error| InventoryError::Unexpected(run_fault(error, BlobIds::default())))?;
        let mut missing = Vec::new();
        for id in &ids {
            let present = tokio::fs::try_exists(blob_path(&self.root, *id))
                .await
                .map_err(|error| {
                    InventoryError::Unexpected(io_fault(
                        &error,
                        BlobIds {
                            blob_id: Some(*id),
                            account_id: None,
                        },
                    ))
                })?;
            if !present {
                missing.push(*id);
            }
        }
        let full = ids.len() >= usize::try_from(limit).unwrap_or(usize::MAX);
        Ok(MissingPage {
            missing,
            next: ids.last().copied().filter(|_| full),
        })
    }
}

#[async_trait]
impl contracts::BlobInventory for BlobInventory {
    /// Each call checks the next `batch` committed rows after the previous call's last row and returns the ids
    /// among them whose file is absent. After the last row the next call starts again at the first, so
    /// `ceil(rows / batch)` calls cover every committed row. A failed call leaves the position where it was.
    async fn find_missing(&self, batch: u32) -> Result<Vec<Uuid>, InventoryError> {
        let mut cursor = self.cursor.lock().await;
        let page = self.scan_page(*cursor, batch).await?;
        *cursor = page.next;
        Ok(page.missing)
    }
}
