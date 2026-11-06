//! Staging of one file: read it, check it against the hash the batch entry carries, send it.

use std::sync::Arc;

use bytes::Bytes;
use sync_core_proto::{FileHash, Hash32, hash_block, rollup};
use tokio::task::JoinError;

use super::wire::stage_small;
use super::{SMALL_FILE_MAX, SmallFile};
use crate::error::SyncError;
use crate::host::FileSource;
use crate::request::RequestPath;

/// A file of the run with the size the planner uses.
pub(super) struct Member {
    pub(super) file: SmallFile,
    pub(super) size: u64,
}

/// Runs a blocking host callback off the async threads. A panic in it propagates; a cancelled task is `Cancelled`.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, SyncError> {
    joined(tokio::task::spawn_blocking(work).await)
}

pub(super) fn joined<T>(result: Result<T, JoinError>) -> Result<T, SyncError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(_) => Err(SyncError::Cancelled),
        },
    }
}

/// Reads the whole file and sends it. The bytes must still hash to `entry.file_hash`; a file that changed
/// since it was hashed is `SourceChanged` and nothing is sent.
pub(super) async fn stage_member(path: &RequestPath, member: &Member) -> Result<(), SyncError> {
    let source = Arc::clone(&member.file.source);
    let expected = member.file.entry.file_hash;
    let (bytes, hash) = blocking(move || read_checked(source.as_ref(), &expected)).await??;
    let (key, sent) = (member.file.entry.staged_key, bytes.len() as u64);
    let staged = stage_small(path, key, bytes, &hash).await?;
    if staged.staged_key != key || staged.size != sent {
        return Err(SyncError::ProtocolViolation {
            detail: "staged row does not match the upload",
        });
    }
    Ok(())
}

fn read_checked(
    source: &dyn FileSource,
    expected: &FileHash,
) -> Result<(Bytes, Hash32), SyncError> {
    let io = |error: std::io::Error| SyncError::Io { kind: error.kind() };
    let len = source.len().map_err(io)?;
    if len > SMALL_FILE_MAX {
        return Err(SyncError::SourceChanged);
    }
    let bytes = if len == 0 {
        Bytes::new()
    } else {
        let wanted = u32::try_from(len).map_err(|_| SyncError::SourceChanged)?;
        source.read_at(0, wanted).map_err(io)?
    };
    if bytes.len() as u64 != len {
        return Err(SyncError::SourceChanged);
    }
    let hash = hash_block(&bytes).map_err(|_| SyncError::SourceChanged)?;
    let actual = if bytes.is_empty() {
        rollup(&[])
    } else {
        rollup(&[hash])
    };
    if &actual != expected {
        return Err(SyncError::SourceChanged);
    }
    Ok((bytes, hash))
}
