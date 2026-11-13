use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use contracts::BlobHandle;
use sc_clock::Timestamp;
use sc_ids::Hash32;
use thiserror::Error;
use uuid::Uuid;

use crate::checkpoint::{CheckpointMark, CheckpointState};

const HASH_LEN: usize = 32;

/// A refused progress operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProgressError {
    #[error("block index {index} is outside the {block_count} blocks of the session")]
    OutOfRange { index: u32, block_count: u32 },
    /// The block is already received under another hash; the stored hash is kept.
    #[error("block {index} is already received with another hash")]
    HashConflict { index: u32 },
}

/// A checkpoint row that cannot describe a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CheckpointError {
    #[error("received bitmap has {found} bytes, {expected} expected for the block count")]
    ReceivedLength { expected: usize, found: usize },
    #[error("block hashes have {found} bytes, {expected} expected for the block count")]
    HashesLength { expected: usize, found: usize },
    #[error("received bitmap sets a bit beyond the block count")]
    BitBeyondCount,
}

/// The columns of an `upload_sessions` row that carry progress.
#[derive(Debug, Clone, Copy)]
pub struct Checkpoint<'a> {
    pub block_count: u32,
    pub received: &'a [u8],
    pub block_hashes: &'a [u8],
}

/// How a block that arrives for `index` with an expected hash relates to what the session holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStart {
    /// The block is now in flight on a free lane.
    Started,
    /// Already received with this hash: a repeat, acknowledged without a write.
    Received,
    /// Already received with another hash.
    ReceivedOther,
    /// In flight with this hash on another lane; not yet durable, so not acknowledged.
    InFlight,
    /// In flight with another hash.
    InFlightOther,
    /// Every lane of the session is busy.
    LanesFull,
}

/// Progress of one session. Bit `i` of the `received` bitset is bit `i % 8`, least significant first, of byte
/// `i / 8`, and `received` and the hash bytes have the layout of the `upload_sessions` columns, so a checkpoint
/// writes them as they are. The blocks in flight (at most one per lane) are kept with their expected hashes.
#[derive(Debug)]
pub struct SessionProgress {
    block_count: u32,
    received: Vec<u8>,
    in_flight: Vec<(u32, Hash32)>,
    block_hashes: Vec<u8>,
}

fn bitmap_len(block_count: u32) -> usize {
    (block_count as usize).div_ceil(8)
}

fn get_bit(bits: &[u8], index: u32) -> bool {
    bits[index as usize / 8] & (1 << (index % 8)) != 0
}

fn set_bit(bits: &mut [u8], index: u32) {
    bits[index as usize / 8] |= 1 << (index % 8);
}

fn count_bits(bits: &[u8]) -> u32 {
    bits.iter().map(|byte| byte.count_ones()).sum()
}

impl SessionProgress {
    /// A new session: nothing received, nothing in flight.
    pub fn new(block_count: u32) -> Self {
        SessionProgress {
            block_count,
            received: vec![0; bitmap_len(block_count)],
            in_flight: Vec::new(),
            block_hashes: vec![0; block_count as usize * HASH_LEN],
        }
    }

    /// The progress a checkpoint row recorded; nothing is in flight.
    pub fn from_checkpoint(checkpoint: &Checkpoint<'_>) -> Result<Self, CheckpointError> {
        let Checkpoint {
            block_count,
            received,
            block_hashes,
        } = *checkpoint;
        let expected = bitmap_len(block_count);
        if received.len() != expected {
            return Err(CheckpointError::ReceivedLength {
                expected,
                found: received.len(),
            });
        }
        let expected = block_count as usize * HASH_LEN;
        if block_hashes.len() != expected {
            return Err(CheckpointError::HashesLength {
                expected,
                found: block_hashes.len(),
            });
        }
        let used = block_count % 8;
        let beyond = used != 0 && received.last().is_some_and(|last| last >> used != 0);
        if beyond {
            return Err(CheckpointError::BitBeyondCount);
        }
        Ok(SessionProgress {
            block_count,
            received: received.to_vec(),
            in_flight: Vec::new(),
            block_hashes: block_hashes.to_vec(),
        })
    }

    pub fn block_count(&self) -> u32 {
        self.block_count
    }

    fn check(&self, index: u32) -> Result<(), ProgressError> {
        if index < self.block_count {
            Ok(())
        } else {
            Err(ProgressError::OutOfRange {
                index,
                block_count: self.block_count,
            })
        }
    }

    pub fn is_received(&self, index: u32) -> Result<bool, ProgressError> {
        self.check(index)?;
        Ok(get_bit(&self.received, index))
    }

    /// The hash of a received block, `None` while the block is missing.
    pub fn hash(&self, index: u32) -> Result<Option<Hash32>, ProgressError> {
        if !self.is_received(index)? {
            return Ok(None);
        }
        let start = index as usize * HASH_LEN;
        let mut bytes = [0u8; HASH_LEN];
        bytes.copy_from_slice(&self.block_hashes[start..start + HASH_LEN]);
        Ok(Some(Hash32::from_bytes(bytes)))
    }

    /// Marks the block in flight with its expected hash; `false` when it already is (the hash is kept).
    pub fn begin_block(&mut self, index: u32, hash: Hash32) -> Result<bool, ProgressError> {
        self.check(index)?;
        if self.in_flight_hash(index).is_some() {
            return Ok(false);
        }
        self.in_flight.push((index, hash));
        Ok(true)
    }

    /// The duplicate and lane rules of a block before any body byte is read: a received or in-flight index
    /// answers by its hash, otherwise the block takes a lane when fewer than `lane_limit` are busy.
    pub fn start_block(
        &mut self,
        index: u32,
        hash: Hash32,
        lane_limit: usize,
    ) -> Result<BlockStart, ProgressError> {
        if let Some(stored) = self.hash(index)? {
            return Ok(if stored == hash {
                BlockStart::Received
            } else {
                BlockStart::ReceivedOther
            });
        }
        if let Some(flying) = self.in_flight_hash(index) {
            return Ok(if flying == hash {
                BlockStart::InFlight
            } else {
                BlockStart::InFlightOther
            });
        }
        if self.in_flight.len() >= lane_limit {
            return Ok(BlockStart::LanesFull);
        }
        self.in_flight.push((index, hash));
        Ok(BlockStart::Started)
    }

    /// Clears the in-flight mark of a block whose transfer failed.
    pub fn release_block(&mut self, index: u32) -> Result<(), ProgressError> {
        self.check(index)?;
        self.in_flight.retain(|(flying, _)| *flying != index);
        Ok(())
    }

    pub fn is_in_flight(&self, index: u32) -> Result<bool, ProgressError> {
        self.check(index)?;
        Ok(self.in_flight_hash(index).is_some())
    }

    fn in_flight_hash(&self, index: u32) -> Option<Hash32> {
        self.in_flight
            .iter()
            .find(|(flying, _)| *flying == index)
            .map(|(_, hash)| *hash)
    }

    /// Records a durable block: sets its bit and hash and clears its in-flight bit. Returns `true` when the
    /// block was new and `false` for a repeat with the same hash; a repeat with another hash changes nothing.
    pub fn mark_received(&mut self, index: u32, hash: Hash32) -> Result<bool, ProgressError> {
        self.check(index)?;
        let start = index as usize * HASH_LEN;
        let slot = start..start + HASH_LEN;
        if get_bit(&self.received, index) {
            if self.block_hashes[slot.clone()] != *hash.as_bytes() {
                return Err(ProgressError::HashConflict { index });
            }
            self.in_flight.retain(|(flying, _)| *flying != index);
            return Ok(false);
        }
        self.block_hashes[slot].copy_from_slice(hash.as_bytes());
        set_bit(&mut self.received, index);
        self.in_flight.retain(|(flying, _)| *flying != index);
        Ok(true)
    }

    pub fn received_count(&self) -> u32 {
        count_bits(&self.received)
    }

    pub fn in_flight_count(&self) -> u32 {
        u32::try_from(self.in_flight.len()).unwrap_or(u32::MAX)
    }

    /// The indexes of the blocks not yet received, ascending.
    pub fn missing_blocks(&self) -> Vec<u32> {
        (0..self.block_count)
            .filter(|index| !get_bit(&self.received, *index))
            .collect()
    }

    /// The `received` column as a checkpoint writes it.
    pub fn received_bytes(&self) -> &[u8] {
        &self.received
    }

    /// The `block_hashes` column as a checkpoint writes it.
    pub fn block_hash_bytes(&self) -> &[u8] {
        &self.block_hashes
    }

    /// Bytes this entry holds: the struct plus the capacity of its three buffers.
    pub fn memory_bytes(&self) -> usize {
        size_of::<Self>()
            + self.received.capacity()
            + self.in_flight.capacity() * size_of::<(u32, Hash32)>()
            + self.block_hashes.capacity()
    }
}

/// What a block write needs of its session, fixed when the entry is created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub account: Uuid,
    pub declared_size: i64,
    pub handle: BlobHandle,
}

/// One session's progress behind its own lock: the four lanes of a session serialise on it, other sessions do
/// not wait for it. The checkpoint bookkeeping has a lock of its own; neither lock is taken while the other is
/// held, and neither is held across an `await`.
#[derive(Debug)]
pub struct SharedProgress {
    info: SessionInfo,
    state: Mutex<SessionProgress>,
    checkpoint: Mutex<CheckpointState>,
}

impl SharedProgress {
    fn new(info: SessionInfo, progress: SessionProgress, mark: CheckpointMark) -> Self {
        SharedProgress {
            info,
            state: Mutex::new(progress),
            checkpoint: Mutex::new(CheckpointState::new(mark)),
        }
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    /// Every operation of [`SessionProgress`] leaves the bitsets and hashes consistent before it can panic, so
    /// the data of a poisoned lock is still valid.
    pub fn lock(&self) -> MutexGuard<'_, SessionProgress> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The `expires_at` of the last checkpoint (or of the start).
    pub fn expires_at(&self) -> Timestamp {
        self.checkpoint().expires_at()
    }

    /// Every [`CheckpointState`] operation is a single assignment group that cannot panic midway.
    pub(crate) fn checkpoint(&self) -> MutexGuard<'_, CheckpointState> {
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The struct (which holds the [`SessionProgress`] inline), the wrapped key and the progress buffers.
    fn memory_bytes(&self) -> usize {
        let progress_buffers = self
            .lock()
            .memory_bytes()
            .saturating_sub(size_of::<SessionProgress>());
        size_of::<Self>() + self.info.handle.plan.wrapped_key.capacity() + progress_buffers
    }
}

/// Progress of every session with a recent block, by upload id. An entry is created from the session's
/// checkpoint row when the session is first touched, never for all sessions at start, and dropped by `remove`.
#[derive(Debug, Default)]
pub struct ProgressTable {
    sessions: RwLock<HashMap<Uuid, Arc<SharedProgress>>>,
}

impl ProgressTable {
    pub fn new() -> Self {
        ProgressTable::default()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<Uuid, Arc<SharedProgress>>> {
        self.sessions.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<Uuid, Arc<SharedProgress>>> {
        self.sessions
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self, upload_id: Uuid) -> Option<Arc<SharedProgress>> {
        self.read().get(&upload_id).cloned()
    }

    /// The entry of the session, built from its checkpoint row when absent. An existing entry wins: memory
    /// holds blocks acknowledged after the row was written.
    pub fn get_or_load(
        &self,
        upload_id: Uuid,
        info: SessionInfo,
        mark: CheckpointMark,
        checkpoint: &Checkpoint<'_>,
    ) -> Result<Arc<SharedProgress>, CheckpointError> {
        if let Some(found) = self.get(upload_id) {
            return Ok(found);
        }
        let mut sessions = self.write();
        if let Some(found) = sessions.get(&upload_id) {
            return Ok(Arc::clone(found));
        }
        let loaded = Arc::new(SharedProgress::new(
            info,
            SessionProgress::from_checkpoint(checkpoint)?,
            mark,
        ));
        sessions.insert(upload_id, Arc::clone(&loaded));
        Ok(loaded)
    }

    /// The entry of a new session with nothing received. An existing entry for the id is replaced.
    pub fn insert_new(
        &self,
        upload_id: Uuid,
        info: SessionInfo,
        block_count: u32,
        mark: CheckpointMark,
    ) -> Arc<SharedProgress> {
        let created = Arc::new(SharedProgress::new(
            info,
            SessionProgress::new(block_count),
            mark,
        ));
        self.write().insert(upload_id, Arc::clone(&created));
        created
    }

    /// Drops the entry; `false` when there was none.
    pub fn remove(&self, upload_id: Uuid) -> bool {
        self.write().remove(&upload_id).is_some()
    }

    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    /// Bytes held by every entry, summed from their buffers.
    pub fn memory_bytes(&self) -> usize {
        self.read()
            .values()
            .map(|session| session.memory_bytes())
            .sum()
    }
}
