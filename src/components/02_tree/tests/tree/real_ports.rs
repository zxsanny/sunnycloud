//! What the integration pass (AZ-3720) adds around the real counterparts, none of which stands in for one: a
//! request body, a fault injected after the real effects (the composition root's effect slot of 03), and two
//! observers that delegate to the real `TreeCommit` and `ThumbnailStateSource` and record what they returned or
//! were asked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use contracts::{
    Begin, ByteStream, CommitActor, CommitEffect, EffectError, EntryOutcome, PublishEntry,
    PublishResult, PublishedEntry, ThumbState, ThumbnailStateError, ThumbnailStateSource,
    TreeCommit, TreeCommitError, async_trait,
};
use media_api::ThumbnailStates;
use sc_clock::Timestamp;
use sc_db::DbTx;
use sc_error::ErrorCode;
use sc_ids::{FileHash, hash_block, rollup};
use sunnycloud_tree::PgTreeService;
use uuid::Uuid;

/// A whole body in one chunk.
pub fn body(data: &[u8]) -> ByteStream {
    Box::pin(futures_util::stream::iter([Ok(Bytes::from(data.to_vec()))]))
}

/// The file hash 03 computes for content of at most one block: the roll-up of its one block hash.
pub fn file_hash(data: &[u8]) -> FileHash {
    rollup(&[hash_block(data)])
}

/// A JPEG head followed by `seed`, so every seed is content of its own that 03 sniffs as `image/jpeg`.
pub fn jpeg(seed: &str) -> Vec<u8> {
    let mut data = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00".to_vec();
    data.extend_from_slice(seed.as_bytes());
    data
}

/// The fault the injected effect raises on the next calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// `on_entry` of the entry with this content fails with `Entry(upload_session_gone)`.
    Entry(FileHash),
    /// `on_batch` fails with `Batch(internal)`.
    Batch,
}

/// The effect the composition root may append after 03's own (`UploadPorts::commit_effects`), here to inject a
/// failure after the real 03 and 08 effects ran. It runs no statement.
#[derive(Default)]
pub struct Faults(Mutex<Option<Fault>>);

impl Faults {
    pub fn set(&self, fault: Option<Fault>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = fault;
    }

    fn current(&self) -> Option<Fault> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl CommitEffect for Faults {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Proceed)
    }

    async fn on_entry(&self, _tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        match self.current() {
            Some(Fault::Entry(hash)) if hash == entry.content_hash => {
                Err(EffectError::Entry(ErrorCode::UploadSessionGone))
            }
            _ => Ok(()),
        }
    }

    async fn on_batch(&self, _tx: &mut DbTx, _: &[EntryOutcome]) -> Result<(), EffectError> {
        match self.current() {
            Some(Fault::Batch) => Err(EffectError::Batch(ErrorCode::Internal)),
            _ => Ok(()),
        }
    }
}

/// The real `PgTreeService` as 03 holds it; records per call whether `publish` answered `Replayed`.
pub struct ObservedCommit {
    pub inner: Arc<PgTreeService>,
    replayed: Mutex<Vec<bool>>,
}

impl ObservedCommit {
    pub fn new(inner: Arc<PgTreeService>) -> Self {
        ObservedCommit {
            inner,
            replayed: Mutex::default(),
        }
    }

    pub fn replayed(&self) -> Vec<bool> {
        self.replayed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl TreeCommit for ObservedCommit {
    async fn publish(
        &self,
        actor: CommitActor,
        entries: Vec<PublishEntry>,
        effects: Vec<Arc<dyn CommitEffect>>,
        now: Timestamp,
    ) -> Result<PublishResult, TreeCommitError> {
        let result = self.inner.publish(actor, entries, effects, now).await;
        if let Ok(published) = &result {
            self.replayed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(matches!(published, PublishResult::Replayed(_)));
        }
        result
    }
}

/// The real 08 `ThumbnailStates`; records the hashes of every call.
pub struct ObservedStates {
    pub inner: ThumbnailStates,
    calls: Mutex<Vec<Vec<FileHash>>>,
}

impl ObservedStates {
    pub fn new(inner: ThumbnailStates) -> Self {
        ObservedStates {
            inner,
            calls: Mutex::default(),
        }
    }

    pub fn calls(&self) -> Vec<Vec<FileHash>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl ThumbnailStateSource for ObservedStates {
    async fn states(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(hashes.to_vec());
        self.inner.states(account, hashes).await
    }
}
