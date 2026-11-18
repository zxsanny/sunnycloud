//! SQL of the backup loop over the `asset` and `cursor` tables (17 §4). Every method is one transaction.

use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use sync_core_proto::FileHash;
use uuid::Uuid;

use super::upload_name::with_asset_id;
use super::{CheckpointDb, DbError};
use crate::source::{AssetDescriptor, ChangeToken};

/// States of an asset that still has work: `hashing` is read as `new` (a hash that was never stored).
const PENDING: &str = "state IN ('new', 'hashing', 'uploading')";
const PAUSED_KEY: &str = "paused";
const ERROR_MESSAGE_KEY: &str = "last_error";
const ERROR_ID_KEY: &str = "last_error_id";
const REDACTED_KEY: &str = "location_redacted";
/// The cursor key discovery and `status` share: the access level the last run saw.
pub const ACCESS_LEVEL_KEY: &str = "access_level";
/// Another asset holds this name (ASCII case folded, the index `asset_upload_name_lower`).
const NAME_TAKEN: &str =
    "SELECT 1 FROM asset WHERE lower(upload_name) = lower(?1) AND asset_id != ?2 LIMIT 1";
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

/// How an asset ended for good (until its change token moves): the `skipped_*` and `failed` states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetEnd {
    Gone,
    Name,
    Size,
    Failed,
}

impl AssetEnd {
    fn state(self) -> &'static str {
        match self {
            Self::Gone => "skipped_gone",
            Self::Name => "skipped_name",
            Self::Size => "skipped_size",
            Self::Failed => "failed",
        }
    }
}

/// Assets per state. `visible` is every stored asset, counted apart from the states, so the sum rule of the
/// contract is checked against the rows and not assumed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetCounts {
    pub visible: u64,
    pub uploaded: u64,
    pub pending: u64,
    pub gone: u64,
    pub name: u64,
    pub size: u64,
    pub failed: u64,
}

/// The message and correlation id of the last error, as `status` shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredError {
    pub message: String,
    pub correlation_id: String,
}

#[derive(Clone, Debug)]
pub struct AssetStore {
    db: Arc<CheckpointDb>,
}

fn id_bytes(id: Option<Uuid>) -> Option<Vec<u8>> {
    id.map(|id| id.as_bytes().to_vec())
}

/// The upload name is chosen when the row is stored and kept in it: a name another asset holds gets the asset id.
/// The result is only used for a row that is new or whose token changed.
fn chosen_name(
    taken: &mut rusqlite::Statement<'_>,
    descriptor: &AssetDescriptor,
) -> Result<String, DbError> {
    let held = taken.exists(params![descriptor.display_name, descriptor.asset_id])?;
    Ok(if held {
        with_asset_id(&descriptor.display_name, descriptor.asset_id)
    } else {
        descriptor.display_name.clone()
    })
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

    /// Stores listed assets as `new`, each under its chosen upload name. An asset already stored under another token was edited: its hash and upload
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
            let mut taken = transaction.prepare(NAME_TAKEN)?;
            for descriptor in descriptors {
                upsert.execute(params![
                    descriptor.asset_id,
                    descriptor.change_token.0,
                    size_of(descriptor)?,
                    descriptor.date_taken,
                    chosen_name(&mut taken, descriptor)?,
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
            let mut taken = transaction.prepare(NAME_TAKEN)?;
            for descriptor in descriptors {
                insert.execute(params![
                    descriptor.asset_id,
                    descriptor.change_token.0,
                    size_of(descriptor)?,
                    descriptor.date_taken,
                    chosen_name(&mut taken, descriptor)?,
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

    pub fn mark_ended(&self, asset_id: i64, end: AssetEnd) -> Result<(), DbError> {
        self.db.write(|transaction| {
            transaction.execute(
                "UPDATE asset SET state = ?2 WHERE asset_id = ?1",
                params![asset_id, end.state()],
            )?;
            Ok(())
        })
    }

    /// Ends the assets whose upload a batch entry refused. An asset edited meanwhile has no key and is not touched.
    pub fn mark_ended_by_key(&self, ended: &[(Uuid, AssetEnd)]) -> Result<(), DbError> {
        self.db.write(|transaction| {
            let mut update = transaction.prepare(
                "UPDATE asset SET state = ?2 WHERE client_key = ?1 AND state = 'uploading'",
            )?;
            for (key, end) in ended {
                update.execute(params![key.as_bytes().to_vec(), end.state()])?;
            }
            Ok(())
        })
    }

    pub fn counts(&self) -> Result<AssetCounts, DbError> {
        self.db.read(|connection| {
            let count =
                |value: i64| u64::try_from(value).map_err(|_| DbError::Malformed("asset count"));
            let mut counts = AssetCounts {
                visible: count(
                    connection.query_row("SELECT count(*) FROM asset", [], |row| row.get(0))?,
                )?,
                ..AssetCounts::default()
            };
            let mut statement =
                connection.prepare("SELECT state, count(*) FROM asset GROUP BY state")?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (state, value) in rows {
                let slot = match state.as_str() {
                    "uploaded" => &mut counts.uploaded,
                    "new" | "hashing" | "uploading" => &mut counts.pending,
                    "skipped_gone" => &mut counts.gone,
                    "skipped_name" => &mut counts.name,
                    "skipped_size" => &mut counts.size,
                    "failed" => &mut counts.failed,
                    _ => return Err(DbError::Malformed("asset state")),
                };
                *slot += count(value)?;
            }
            Ok(counts)
        })
    }

    /// A new run starts: the pause and the last error of the run before are no longer true.
    pub fn begin_run(&self) -> Result<(), DbError> {
        self.db.write(|transaction| {
            transaction.execute(
                "DELETE FROM cursor WHERE key IN (?1, ?2, ?3)",
                params![PAUSED_KEY, ERROR_MESSAGE_KEY, ERROR_ID_KEY],
            )?;
            Ok(())
        })
    }

    pub fn set_paused(&self, reason: &str) -> Result<(), DbError> {
        self.set_cursor_text(PAUSED_KEY, reason)
    }

    pub fn paused(&self) -> Result<Option<String>, DbError> {
        self.cursor_text(PAUSED_KEY)
    }

    pub fn set_last_error(&self, error: &StoredError) -> Result<(), DbError> {
        self.db.write(|transaction| {
            let mut put = transaction
                .prepare("INSERT OR REPLACE INTO cursor (key, value) VALUES (?1, ?2)")?;
            put.execute(params![ERROR_MESSAGE_KEY, error.message])?;
            put.execute(params![ERROR_ID_KEY, error.correlation_id])?;
            Ok(())
        })
    }

    pub fn last_error(&self) -> Result<Option<StoredError>, DbError> {
        let (message, id) = (
            self.cursor_text(ERROR_MESSAGE_KEY)?,
            self.cursor_text(ERROR_ID_KEY)?,
        );
        match (message, id) {
            (Some(message), Some(correlation_id)) => Ok(Some(StoredError {
                message,
                correlation_id,
            })),
            (None, None) => Ok(None),
            _ => Err(DbError::Malformed("last error")),
        }
    }

    pub fn set_location_redacted(&self, redacted: bool) -> Result<(), DbError> {
        self.set_cursor_text(REDACTED_KEY, if redacted { "1" } else { "0" })
    }

    pub fn location_redacted(&self) -> Result<Option<bool>, DbError> {
        match self.cursor_text(REDACTED_KEY)?.as_deref() {
            None => Ok(None),
            Some("1") => Ok(Some(true)),
            Some("0") => Ok(Some(false)),
            Some(_) => Err(DbError::Malformed("location flag")),
        }
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
