//! Rotation by rewrap: the data keys of existing blobs move from one master key to another, the files stay as
//! they are.
//!
//! A batch is two short transactions around the cipher work: one reads up to `n` rows wrapped under the old
//! key, one writes the new wraps with a guard on the wrap that was read. No lock is held while the keys are
//! opened and sealed, reads of the blobs go on throughout (a reader sees the old or the new wrap, and both
//! unwrap while both keys are in the ring), and a run that stops half way is continued by calling again.

use contracts::KeyError;
use sc_observability::log_info;

use super::{MasterKeyring, blob_ids, keyring_repo};
use crate::error::{BlobIds, key_error_from_cipher, row_fault};

impl MasterKeyring {
    /// Re-wraps up to `n` rows from `from_id` to `to_id` and returns how many it handled; 0 means no row is left
    /// under `from_id`. A row that another run rewrapped or the collector removed meanwhile counts as handled.
    /// Both ids must be in the ring and differ, else `Unexpected`; a wrap that does not open under `from_id`
    /// stops the batch with `Unexpected` and changes nothing. The keyring is loaded and verified first if it is
    /// not yet.
    pub async fn rewrap_batch(&self, from_id: i16, to_id: i16, n: u32) -> Result<u64, KeyError> {
        self.load_and_verify().await?;
        let held = self.ring.get().and_then(|ring| {
            let from = ring.get(from_id)?;
            let to = ring.get(to_id)?;
            Some((from, to))
        });
        let Some((from, to)) = held.filter(|_| from_id != to_id) else {
            return Err(KeyError::Unexpected(row_fault(
                "rewrap key ids",
                BlobIds::default(),
            )));
        };
        if n == 0 {
            return Ok(0);
        }
        let rows = keyring_repo::under_key(&self.db, from_id, i64::from(n)).await?;
        if rows.is_empty() {
            log_info!("rewrap finished, no row left under the old key");
            return Ok(0);
        }
        let mut done = keyring_repo::Rewrapped {
            ids: Vec::with_capacity(rows.len()),
            old_wraps: Vec::with_capacity(rows.len()),
            new_wraps: Vec::with_capacity(rows.len()),
        };
        for (blob_id, wrapped) in rows {
            let aad = blob_id.as_bytes();
            let data_key = from
                .open(aad, &wrapped)
                .map_err(|error| key_error_from_cipher(error, blob_ids(blob_id)))?;
            let rewrapped = to
                .seal(aad, &data_key)
                .map_err(|error| key_error_from_cipher(error, blob_ids(blob_id)))?;
            done.ids.push(blob_id);
            done.old_wraps.push(wrapped);
            done.new_wraps.push(rewrapped);
        }
        keyring_repo::replace_wraps(&self.db, &done, from_id, to_id).await?;
        log_info!("rewrap batch finished");
        Ok(u64::try_from(done.ids.len()).unwrap_or(u64::MAX))
    }
}
