use rusqlite::{Connection, OptionalExtension, Row};
use sync_core::{BatchRecord, UploadCheckpoint, UploadState};
use sync_core_proto::{FileHash, Hash32};
use uuid::Uuid;

use super::entries::conversion_failure;

pub(in crate::local_index) fn save(
    conn: &Connection,
    local_key: &str,
    checkpoint: &UploadCheckpoint,
) -> rusqlite::Result<()> {
    let blocklist: Vec<u8> = checkpoint
        .block_hashes
        .iter()
        .flat_map(|hash| *hash.as_bytes())
        .collect();
    conn.prepare_cached(
        "INSERT INTO upload_checkpoint (local_key, session_id, commit_id, file_hash, state, blocklist) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (local_key) DO UPDATE SET session_id = excluded.session_id, commit_id = excluded.commit_id, \
         file_hash = excluded.file_hash, state = excluded.state, blocklist = excluded.blocklist",
    )?
    .execute(rusqlite::params![
        local_key,
        checkpoint.session_id.map(|id| id.to_string()),
        checkpoint.commit_id.to_string(),
        checkpoint.file_hash.as_ref().map(|hash| hash.as_bytes().as_slice()),
        state_text(checkpoint.state),
        blocklist,
    ])?;
    Ok(())
}

pub(in crate::local_index) fn load(
    conn: &Connection,
    local_key: &str,
) -> rusqlite::Result<Option<UploadCheckpoint>> {
    conn.prepare_cached(
        "SELECT session_id, commit_id, file_hash, state, blocklist FROM upload_checkpoint WHERE local_key = ?1",
    )?
    .query_row([local_key], checkpoint_from_row)
    .optional()
}

pub(in crate::local_index) fn clear(conn: &Connection, local_key: &str) -> rusqlite::Result<()> {
    conn.prepare_cached("DELETE FROM upload_checkpoint WHERE local_key = ?1")?
        .execute([local_key])?;
    Ok(())
}

pub(in crate::local_index) fn save_batch(
    conn: &Connection,
    record: &BatchRecord,
) -> rusqlite::Result<()> {
    let entries = serde_json::to_string(&record.entries)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.prepare_cached(
        "INSERT INTO batch (commit_id, entries) VALUES (?1, ?2) \
         ON CONFLICT (commit_id) DO UPDATE SET entries = excluded.entries",
    )?
    .execute(rusqlite::params![record.commit_id.to_string(), entries])?;
    Ok(())
}

pub(in crate::local_index) fn load_batches(
    conn: &Connection,
) -> rusqlite::Result<Vec<BatchRecord>> {
    conn.prepare_cached("SELECT commit_id, entries FROM batch ORDER BY commit_id")?
        .query_map([], batch_from_row)?
        .collect()
}

pub(in crate::local_index) fn clear_batch(
    conn: &Connection,
    commit_id: Uuid,
) -> rusqlite::Result<()> {
    conn.prepare_cached("DELETE FROM batch WHERE commit_id = ?1")?
        .execute([commit_id.to_string()])?;
    Ok(())
}

const fn state_text(state: UploadState) -> &'static str {
    match state {
        UploadState::Hashing => "hashing",
        UploadState::Uploading => "uploading",
        UploadState::Committing => "committing",
    }
}

fn parse_state(text: &str) -> Option<UploadState> {
    Some(match text {
        "hashing" => UploadState::Hashing,
        "uploading" => UploadState::Uploading,
        "committing" => UploadState::Committing,
        _ => return None,
    })
}

fn parse_uuid(text: &str, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(text).map_err(|_| conversion_failure(column, "uuid"))
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<UploadCheckpoint> {
    let session_id = row
        .get::<_, Option<String>>(0)?
        .map(|text| parse_uuid(&text, 0))
        .transpose()?;
    let file_hash = row
        .get::<_, Option<Vec<u8>>>(2)?
        .map(|bytes| {
            <[u8; 32]>::try_from(bytes.as_slice())
                .map(FileHash::from_bytes)
                .map_err(|_| conversion_failure(2, "file hash"))
        })
        .transpose()?;
    let state: String = row.get(3)?;
    let blocklist: Vec<u8> = row.get(4)?;
    let (hashes, rest) = blocklist.as_chunks::<32>();
    if !rest.is_empty() {
        return Err(conversion_failure(4, "blocklist"));
    }
    Ok(UploadCheckpoint {
        session_id,
        block_hashes: hashes.iter().copied().map(Hash32::from_bytes).collect(),
        file_hash,
        commit_id: parse_uuid(&row.get::<_, String>(1)?, 1)?,
        state: parse_state(&state).ok_or_else(|| conversion_failure(3, "upload state"))?,
    })
}

fn batch_from_row(row: &Row<'_>) -> rusqlite::Result<BatchRecord> {
    let entries: String = row.get(1)?;
    Ok(BatchRecord {
        commit_id: parse_uuid(&row.get::<_, String>(0)?, 0)?,
        entries: serde_json::from_str(&entries)
            .map_err(|_| conversion_failure(1, "batch entries"))?,
    })
}
