//! `hash_file` (`sync_api.md`): a host file read one 2 MiB block at a time, hashed on the blocking pool, with the
//! hash state handed to the host every `sync.hash_checkpoint_blocks` blocks and the CPU use held to
//! `sync.hash_cpu_share`. One block buffer is alive at a time; the rest is 32 bytes per block of hash state.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use sc_clock::Clock;
use sync_core_proto::{BLOCK_SIZE, FileHash, Hash32, ResumeHashState, rollup};
use uuid::Uuid;

use crate::config::SyncConfig;
use crate::error::SyncError;
use crate::host::{
    CheckpointStore, FileIdentity, FileSource, ProgressSink, UploadCheckpoint, UploadState,
};
use crate::request::RequestPath;
use crate::small::blocking;

const BLOCK: u64 = BLOCK_SIZE as u64;

/// The blocks and the whole-file hash of a file that did not change while it was hashed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashedFile {
    pub block_hashes: Vec<Hash32>,
    pub file_hash: FileHash,
}

/// Where the hash state goes while hashing runs: the host's store, under the key of the file's upload.
/// Each hand-over is an [`UploadCheckpoint`] in state `Hashing` holding the blocks hashed so far.
#[derive(Clone)]
pub struct HashCheckpoint {
    pub store: Arc<dyn CheckpointStore>,
    pub local_key: String,
    /// The upload's `client_key` (`UploadCheckpoint::commit_id`).
    pub commit_id: Uuid,
}

/// The hashing settings and the clock that measures and paces them.
pub(crate) struct Hashing {
    clock: Arc<dyn Clock>,
    cpu_share: u8,
    checkpoint_blocks: u32,
}

impl Hashing {
    pub(crate) fn new(clock: Arc<dyn Clock>, config: &SyncConfig) -> Self {
        Self {
            clock,
            cpu_share: config.hash_cpu_share.clamp(1, 100),
            checkpoint_blocks: config.hash_checkpoint_blocks.max(1),
        }
    }

    /// Hashes `source`, continuing after `resume.blocks_done` blocks (only later blocks are read). The state is
    /// handed to `checkpoint` after every `sync.hash_checkpoint_blocks`-th block, except the last. A failed
    /// hand-over is logged: the next one or the final result still stands. `progress` counts hashed bytes.
    /// The file's identity is read at the start and the end; a difference is `SourceChanged` and no result.
    pub(crate) async fn hash_file(
        &self,
        path: &RequestPath,
        source: Arc<dyn FileSource>,
        resume: Option<ResumeHashState>,
        progress: Arc<dyn ProgressSink>,
        checkpoint: HashCheckpoint,
    ) -> Result<HashedFile, SyncError> {
        path.check()?;
        let opening = identity_of(&source).await?;
        let len = blocking_io({
            let source = Arc::clone(&source);
            move || source.len()
        })
        .await?;
        let total_blocks = len.div_ceil(BLOCK);
        let mut hashes = resumed_hashes(resume, total_blocks)?;
        for index in hashes.len() as u64..total_blocks {
            path.check()?;
            let offset = index * BLOCK;
            let want = u32::try_from((len - offset).min(BLOCK)).unwrap_or(BLOCK_SIZE as u32);
            let (hash, busy) = {
                let (source, clock) = (Arc::clone(&source), Arc::clone(&self.clock));
                blocking(move || read_and_hash(source.as_ref(), clock.as_ref(), offset, want))
                    .await??
            };
            hashes.push(hash);
            progress.on_progress(offset + u64::from(want), len);
            let done = index + 1;
            if done < total_blocks && done.is_multiple_of(u64::from(self.checkpoint_blocks)) {
                hand_over(&checkpoint, &hashes).await?;
            }
            path.pause(throttle_delay(busy, self.cpu_share)).await?;
        }
        if identity_of(&source).await? != opening {
            return Err(SyncError::SourceChanged);
        }
        let file_hash = rollup(&hashes);
        Ok(HashedFile {
            block_hashes: hashes,
            file_hash,
        })
    }
}

/// The hashes the run starts with. A state that does not describe its own blocks is a caller bug; a state
/// that reaches past the end of the file means the file shrank since it was saved.
fn resumed_hashes(
    resume: Option<ResumeHashState>,
    total_blocks: u64,
) -> Result<Vec<Hash32>, SyncError> {
    let Some(state) = resume else {
        return Ok(Vec::new());
    };
    if state.block_hashes.len() as u64 != state.blocks_done {
        return Err(SyncError::Io {
            kind: io::ErrorKind::InvalidInput,
        });
    }
    if state.blocks_done > total_blocks {
        return Err(SyncError::SourceChanged);
    }
    Ok(state.block_hashes)
}

/// Reads one block and hashes it; the second value is the time the hashing itself took. Fewer bytes than the
/// file's length promised means the file shrank.
fn read_and_hash(
    source: &dyn FileSource,
    clock: &dyn Clock,
    offset: u64,
    want: u32,
) -> Result<(Hash32, Duration), SyncError> {
    let bytes = source.read_at(offset, want).map_err(io_error)?;
    if bytes.len() as u64 != u64::from(want) {
        return Err(SyncError::SourceChanged);
    }
    let started = clock.monotonic();
    let hash = sc_ids::hash_block(&bytes);
    Ok((hash, clock.monotonic().saturating_duration_since(started)))
}

/// How long to rest after `busy` of hashing so that hashing uses `share` percent of one core:
/// `busy * (100 - share) / share`.
fn throttle_delay(busy: Duration, share: u8) -> Duration {
    if share >= 100 {
        return Duration::ZERO;
    }
    busy.saturating_mul(u32::from(100 - share)) / u32::from(share)
}

async fn hand_over(checkpoint: &HashCheckpoint, hashes: &[Hash32]) -> Result<(), SyncError> {
    let (store, key) = (Arc::clone(&checkpoint.store), checkpoint.local_key.clone());
    let record = UploadCheckpoint {
        session_id: None,
        block_hashes: hashes.to_vec(),
        file_hash: None,
        commit_id: checkpoint.commit_id,
        state: UploadState::Hashing,
    };
    if blocking(move || store.save(&key, record)).await?.is_err() {
        tracing::warn!("hash state was not stored, hashing continues");
    }
    Ok(())
}

pub(crate) async fn identity_of(source: &Arc<dyn FileSource>) -> Result<FileIdentity, SyncError> {
    let source = Arc::clone(source);
    blocking_io(move || source.identity()).await
}

async fn blocking_io<T: Send + 'static>(
    call: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T, SyncError> {
    blocking(call).await?.map_err(io_error)
}

fn io_error(error: io::Error) -> SyncError {
    SyncError::Io { kind: error.kind() }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::throttle_delay;

    #[test]
    fn rests_three_times_the_hashing_time_at_a_quarter_core() {
        // Arrange
        let busy = Duration::from_millis(8);

        // Act
        let delay = throttle_delay(busy, 25);

        // Assert
        assert_eq!(delay, Duration::from_millis(24));
    }

    #[test]
    fn does_not_rest_at_the_full_core_and_rests_99_times_at_one_percent() {
        // Arrange
        let busy = Duration::from_millis(2);

        // Act
        let full = throttle_delay(busy, 100);
        let one_percent = throttle_delay(busy, 1);

        // Assert
        assert_eq!(full, Duration::ZERO);
        assert_eq!(one_percent, Duration::from_millis(198));
    }
}
