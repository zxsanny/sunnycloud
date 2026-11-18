//! SQL of the backup loop over the `asset` and `cursor` tables (17 §4). Every method is one transaction.

use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use sync_core_proto::FileHash;
use uuid::Uuid;

use super::{CheckpointDb, DbError};
use crate::source::{AssetDescriptor, ChangeToken};

/// States of an asset that still has work: `hashing` is read as `new` (a hash that was never stored).
const PENDING: &str = "state IN ('new', 'hashing', 'uploading')";
const ID_LEN: usize = 16;
const HASH_LEN: usize = 32;

/// A pending asset. `hashed` is the content hash and `client_key` stored before its upload began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAsset {
    pub asset_id: i64,
    pub upload_name: String,
    pub hashed: Option<(FileHash, Uuid)>,
}

/// The commit acknowledgement of one asset. `NothingToSend` knows no node, a conflict copy no revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acknowledged {
    pub client_key: Uuid,
    pub node_id: Option<Uuid>,
    pub revision_id: Option<Uuid>,
}

#[derive(Clone, Debug)]
pub struct AssetStore {
    db: Arc<CheckpointDb>,
}

fn id_bytes(id: Option<Uuid>) -> Option<Vec<u8>> {
    id.map(|id| id.as_bytes().to_vec())
}

fn size_of(descriptor: &AssetDescriptor) -> Result<i64, DbError> {
    i64::try_from(descriptor.size).map_err(|_| DbError::Malformed("asset size"))
}

impl AssetStore {
    pub fn new(db: Arc<CheckpointDb>) -> Self {
        Self { db }
    }

    pub fn cursor_text(&self, key: &str) -> Result<Option<String>, DbError> {
        self.db.read(|connection| {
            Ok(connection
                .query_row("SELECT value FROM cursor WHERE key = ?1", [key], |row| {
                    row.get(0)
                })
                .optional()?)
        })
    }

    pub fn set_cursor_text(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.db.write(|transaction| {
            transaction.execute(
                "INSERT OR REPLACE INTO cursor (key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
            Ok(())
        })
    }

    /// Token bytes are stored as they came: the column has no text affinity conversion for a blob.
    pub fn cursor_token(&self, key: &str) -> Result<Option<ChangeToken>, DbError> {
        self.db.read(|connection| {
            Ok(connection
                .query_row("SELECT value FROM cursor WHERE key = ?1", [key], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .optional()?
                .map(ChangeToken))
        })
    }

    pub fn set_cursor_token(&self, key: &str, token: &ChangeToken) -> Result<(), DbError> {
        self.db.write(|transaction| {
            transaction.execute(
                "INSERT OR REPLACE INTO cursor (key, value) VALUES (?1, ?2)",
                params![key, token.0],
            )?;
            Ok(())
        })
    }

    /// Stores listed assets as `new`. An asset already stored under another token was edited: its hash and upload
    /// ids are dropped and it is `new` again. The same token again changes nothing.
    pub fn store_listed(&self, descriptors: &[AssetDescriptor]) -> Result<(), DbError> {
        self.db.write(|transaction| {
            let mut upsert = transaction.prepare(
                "INSERT INTO asset (asset_id, change_token, size, date_taken, upload_name) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT (asset_id) DO UPDATE SET change_token = excluded.change_token, \
                 size = excluded.size, date_taken = excluded.date_taken, upload_name = excluded.upload_name, \
                 content_hash = NULL, client_key = NULL, node_id = NULL, revision_id = NULL, state = 'new' \
                 WHERE asset.change_token IS NOT excluded.change_token",
            )?;
            for descriptor in descriptors {
                upsert.execute(params![
                    descriptor.asset_id,
                    descriptor.change_token.0,
                    size_of(descriptor)?,
                    descriptor.date_taken,
                    descriptor.display_name,
                ])?;
            }
            Ok(())
        })
    }

    /// Stores assets of a visible-id diff as `new`; a stored asset is left as it is.
    pub fn store_unknown(&self, descriptors: &[AssetDescriptor]) -> Result<(), DbError> {
        self.db.write(|transaction| {
            let mut insert = transaction.prepare(
                "INSERT OR IGNORE INTO asset (asset_id, change_token, size, date_taken, upload_name) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for descriptor in descriptors {
                insert.execute(params![
                    descriptor.asset_id,
                    descriptor.change_token.0,
                    size_of(descriptor)?,
                    descriptor.date_taken,
                    descriptor.display_name,
                ])?;
            }
            Ok(())
        })
    }

    /// The ids of `ids` that have no row.
    pub fn unknown_ids(&self, ids: &[i64]) -> Result<Vec<i64>, DbError> {
        self.db.read(|connection| {
            let mut lookup = connection.prepare("SELECT 1 FROM asset WHERE asset_id = ?1")?;
            let mut unknown = Vec::new();
            for id in ids {
                if !lookup.exists([id])? {
                    unknown.push(*id);
                }
            }
            Ok(unknown)
        })
    }

    /// Up to `limit` pending assets with an id above `after`, in id order.
    pub fn pending_after(&self, after: i64, limit: usize) -> Result<Vec<PendingAsset>, DbError> {
        self.db.read(|connection| {
            let mut statement = connection.prepare(&format!(
                "SELECT asset_id, upload_name, state, content_hash, client_key FROM asset \
                 WHERE asset_id > ?1 AND {PENDING} ORDER BY asset_id LIMIT ?2"
            ))?;
            let rows = statement
                .query_map(params![after, limit as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<Vec<u8>>>(3)?,
                        row.get::<_, Option<Vec<u8>>>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().map(pending_of).collect()
        })
    }

    pub fn pending_count(&self) -> Result<u64, DbError> {
        self.db.read(|connection| {
            let count: i64 = connection.query_row(
                &format!("SELECT count(*) FROM asset WHERE {PENDING}"),
                [],
                |row| row.get(0),
            )?;
            u64::try_from(count).map_err(|_| DbError::Malformed("asset count"))
        })
    }

    /// The content hash and key become durable before the asset's upload starts.
    pub fn mark_hashed(
        &self,
        asset_id: i64,
        file_hash: &FileHash,
        client_key: Uuid,
    ) -> Result<(), DbError> {
        self.db.write(|transaction| {
            transaction.execute(
                "UPDATE asset SET content_hash = ?2, client_key = ?3, state = 'uploading' WHERE asset_id = ?1",
                params![asset_id, file_hash.as_bytes().to_vec(), client_key.as_bytes().to_vec()],
            )?;
            Ok(())
        })
    }

    /// Marks the acknowledged assets `uploaded` in one transaction. An asset edited meanwhile has no key and is
    /// not touched.
    pub fn mark_uploaded(&self, done: &[Acknowledged]) -> Result<(), DbError> {
        self.db.write(|transaction| {
            let mut update = transaction.prepare(
                "UPDATE asset SET state = 'uploaded', node_id = ?2, revision_id = ?3 \
                 WHERE client_key = ?1 AND state = 'uploading'",
            )?;
            for ack in done {
                update.execute(params![
                    ack.client_key.as_bytes().to_vec(),
                    id_bytes(ack.node_id),
                    id_bytes(ack.revision_id),
                ])?;
            }
            Ok(())
        })
    }

    pub fn mark_skipped_gone(&self, asset_id: i64) -> Result<(), DbError> {
        self.db.write(|transaction| {
            transaction.execute(
                "UPDATE asset SET state = 'skipped_gone' WHERE asset_id = ?1",
                [asset_id],
            )?;
            Ok(())
        })
    }

    pub fn asset_of_key(&self, client_key: Uuid) -> Result<Option<i64>, DbError> {
        self.db.read(|connection| {
            Ok(connection
                .query_row(
                    "SELECT asset_id FROM asset WHERE client_key = ?1",
                    [client_key.as_bytes().to_vec()],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }
}

/// `asset_id`, `upload_name`, `state`, `content_hash`, `client_key` as stored.
type PendingColumns = (
    i64,
    Option<String>,
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);

fn pending_of(
    (asset_id, upload_name, state, hash, key): PendingColumns,
) -> Result<PendingAsset, DbError> {
    let hashed = match (state.as_str(), hash, key) {
        ("uploading", Some(hash), Some(key)) => {
            let hash = <[u8; HASH_LEN]>::try_from(hash.as_slice())
                .map_err(|_| DbError::Malformed("content hash"))?;
            let key = <[u8; ID_LEN]>::try_from(key.as_slice())
                .map_err(|_| DbError::Malformed("client key"))?;
            Some((FileHash::from(hash), Uuid::from_bytes(key)))
        }
        _ => None,
    };
    Ok(PendingAsset {
        asset_id,
        upload_name: upload_name.ok_or(DbError::Malformed("upload name"))?,
        hashed,
    })
}
