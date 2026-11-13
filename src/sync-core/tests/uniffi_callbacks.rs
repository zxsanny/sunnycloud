//! AC-3 of AZ-3968 (`uniffi_surface.md` foreign-callback case), Rust side: host implementations written against
//! the foreign twins of `FileSource` and `CheckpointStore` (the traits Kotlin and Swift implement) drive
//! `hash_file` and a checkpoint round trip through [`Foreign`] with the same results as the Rust implementations
//! over the same data. Foreign failures arrive as the typed errors the Rust traits define.
#![cfg(feature = "uniffi")]

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;

use std::collections::{BTreeMap, HashMap};
use std::io::ErrorKind;
use std::sync::{Arc, Mutex, PoisonError};

use hash_support::{BLOCK, GeneratedSource, KEY, Progress, SpyStore, checkpoint_into, pattern};
use support::Script;
use sync_core::{
    BatchRecord, CheckpointStore, FileIdentity, FileSource, Foreign, ForeignCheckpointStore,
    ForeignFileSource, HashCheckpoint, HashedFile, HostError, MemoryCheckpointStore, StoreError,
    SyncApi, SyncError, UploadCheckpoint, UploadState,
};
use sync_core_proto::{BatchEntry, FileHash, Hash32};
use tree_support::connected;
use uuid::Uuid;

/// A foreign-style source over `pattern(len)`; `failing` answers every call with `failing`.
struct PatternSource {
    data: Vec<u8>,
    failing: Option<HostError>,
}

impl PatternSource {
    fn new(len: u64) -> Arc<Self> {
        Arc::new(Self {
            data: pattern(len),
            failing: None,
        })
    }

    fn failing(error: HostError) -> Arc<Self> {
        Arc::new(Self {
            data: Vec::new(),
            failing: Some(error),
        })
    }

    fn answer<T>(&self, value: impl FnOnce() -> T) -> Result<T, HostError> {
        match self.failing {
            Some(error) => Err(error),
            None => Ok(value()),
        }
    }
}

impl ForeignFileSource for PatternSource {
    fn len(&self) -> Result<u64, HostError> {
        self.answer(|| self.data.len() as u64)
    }

    fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, HostError> {
        self.answer(|| {
            let start = usize::try_from(offset).expect("offset fits usize");
            let end = (start + len as usize).min(self.data.len());
            self.data[start..end].to_vec()
        })
    }

    fn identity(&self) -> Result<FileIdentity, HostError> {
        self.answer(|| FileIdentity {
            size: self.data.len() as u64,
            mtime_ns: 1_700_000_000_000_000_000,
        })
    }
}

/// A foreign-style store kept in maps, as a Kotlin or Swift host would keep it in its database.
#[derive(Default)]
struct MapStore {
    checkpoints: Mutex<HashMap<String, UploadCheckpoint>>,
    batches: Mutex<BTreeMap<Uuid, BatchRecord>>,
    failing: bool,
}

impl MapStore {
    fn check(&self) -> Result<(), HostError> {
        if self.failing {
            return Err(HostError::Failed);
        }
        Ok(())
    }
}

impl ForeignCheckpointStore for MapStore {
    fn load(&self, local_key: String) -> Result<Option<UploadCheckpoint>, HostError> {
        self.check()?;
        let checkpoints = self
            .checkpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Ok(checkpoints.get(&local_key).cloned())
    }

    fn save(&self, local_key: String, checkpoint: UploadCheckpoint) -> Result<(), HostError> {
        self.check()?;
        let mut checkpoints = self
            .checkpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        checkpoints.insert(local_key, checkpoint);
        Ok(())
    }

    fn clear(&self, local_key: String) -> Result<(), HostError> {
        self.check()?;
        let mut checkpoints = self
            .checkpoints
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        checkpoints.remove(&local_key);
        Ok(())
    }

    fn save_batch(&self, record: BatchRecord) -> Result<(), HostError> {
        self.check()?;
        let mut batches = self.batches.lock().unwrap_or_else(PoisonError::into_inner);
        batches.insert(record.commit_id, record);
        Ok(())
    }

    fn load_batches(&self) -> Result<Vec<BatchRecord>, HostError> {
        self.check()?;
        let batches = self.batches.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(batches.values().cloned().collect())
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), HostError> {
        self.check()?;
        let mut batches = self.batches.lock().unwrap_or_else(PoisonError::into_inner);
        batches.remove(&commit_id);
        Ok(())
    }
}

fn foreign_source(source: Arc<dyn ForeignFileSource>) -> Arc<dyn FileSource> {
    Arc::new(Foreign(source))
}

fn foreign_store(store: Arc<dyn ForeignCheckpointStore>) -> Arc<dyn CheckpointStore> {
    Arc::new(Foreign(store))
}

async fn hash(
    api: &impl SyncApi,
    source: Arc<dyn FileSource>,
    checkpoint: HashCheckpoint,
) -> Result<HashedFile, SyncError> {
    api.hash_file(source, None, Progress::new(), checkpoint)
        .await
}

fn checkpoint(state: UploadState) -> UploadCheckpoint {
    UploadCheckpoint {
        session_id: Some(Uuid::from_u128(7)),
        block_hashes: vec![Hash32::from([1; 32]), Hash32::from([2; 32])],
        file_hash: Some(FileHash::from([3; 32])),
        commit_id: Uuid::from_u128(42),
        state,
    }
}

fn batch(commit_id: u128) -> BatchRecord {
    BatchRecord {
        commit_id: Uuid::from_u128(commit_id),
        entries: vec![BatchEntry {
            staged_key: Uuid::from_u128(commit_id + 1),
            parent: Uuid::from_u128(1),
            name: "a.jpg".to_owned(),
            base_revision: None,
            file_hash: FileHash::from([4; 32]),
            client_meta: Some(serde_json::json!({"taken": 1})),
            import_hint: None,
        }],
    }
}

/// Every answer of one store over the same sequence of calls, in call order.
type Trace = (
    Vec<Option<UploadCheckpoint>>,
    Vec<Vec<BatchRecord>>,
    Vec<Result<(), StoreError>>,
);

fn round_trip(store: &dyn CheckpointStore) -> Trace {
    let (mut loads, mut batches, mut writes) = (Vec::new(), Vec::new(), Vec::new());
    loads.push(store.load(KEY).expect("load"));
    writes.push(store.save(KEY, checkpoint(UploadState::Hashing)));
    writes.push(store.save(KEY, checkpoint(UploadState::Committing)));
    loads.push(store.load(KEY).expect("load"));
    writes.push(store.save_batch(batch(100)));
    writes.push(store.save_batch(batch(200)));
    batches.push(store.load_batches().expect("load batches"));
    writes.push(store.clear_batch(Uuid::from_u128(100)));
    batches.push(store.load_batches().expect("load batches"));
    writes.push(store.clear(KEY));
    loads.push(store.load(KEY).expect("load"));
    (loads, batches, writes)
}

#[tokio::test]
async fn hash_file_over_a_foreign_source_equals_the_rust_source() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let sizes = [0, 1, BLOCK - 1, BLOCK, BLOCK + 1, 3 * BLOCK];

    for size in sizes {
        // Act
        let rust = hash(
            &api,
            Arc::new(GeneratedSource::new(size)),
            checkpoint_into(&SpyStore::new()),
        )
        .await;
        let foreign = hash(
            &api,
            foreign_source(PatternSource::new(size)),
            checkpoint_into(&SpyStore::new()),
        )
        .await;

        // Assert
        assert_eq!(foreign, rust, "size {size}");
        assert!(rust.is_ok(), "size {size}: {rust:?}");
    }
}

#[tokio::test]
async fn hash_state_handed_to_a_foreign_store_equals_the_rust_store() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let size = 70 * BLOCK;
    let rust_store = Arc::new(MemoryCheckpointStore::new());
    let foreign_backing = Arc::new(MapStore::default());
    let into = |store: Arc<dyn CheckpointStore>| HashCheckpoint {
        store,
        local_key: KEY.to_owned(),
        commit_id: Uuid::from_u128(42),
    };

    // Act
    let rust = hash(
        &api,
        foreign_source(PatternSource::new(size)),
        into(rust_store.clone()),
    )
    .await;
    let foreign = hash(
        &api,
        foreign_source(PatternSource::new(size)),
        into(foreign_store(foreign_backing.clone())),
    )
    .await;

    // Assert
    assert_eq!(foreign, rust);
    let saved = rust_store.load(KEY).expect("rust store answers");
    assert!(saved.is_some(), "70 blocks hand the state over twice");
    assert_eq!(
        Foreign(foreign_backing as Arc<dyn ForeignCheckpointStore>)
            .load(KEY)
            .expect("foreign store answers"),
        saved
    );
}

#[test]
fn checkpoint_round_trip_over_a_foreign_store_equals_the_memory_store() {
    // Arrange
    let rust = MemoryCheckpointStore::new();
    let foreign = Foreign(Arc::new(MapStore::default()) as Arc<dyn ForeignCheckpointStore>);

    // Act
    let rust_trace = round_trip(&rust);
    let foreign_trace = round_trip(&foreign);

    // Assert
    assert_eq!(foreign_trace, rust_trace);
}

#[test]
fn a_failing_foreign_store_answers_store_error() {
    // Arrange
    let store = Foreign(Arc::new(MapStore {
        failing: true,
        ..MapStore::default()
    }) as Arc<dyn ForeignCheckpointStore>);

    // Act
    let answers = [
        store.load(KEY).map(|_| ()),
        store.save(KEY, checkpoint(UploadState::Uploading)),
        store.clear(KEY),
        store.save_batch(batch(1)),
        store.load_batches().map(|_| ()),
        store.clear_batch(Uuid::from_u128(1)),
    ];

    // Assert
    assert!(answers.iter().all(|answer| *answer == Err(StoreError)));
}

#[tokio::test]
async fn foreign_source_failures_become_the_io_kind_of_the_rust_trait() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let cases = [
        (HostError::NotFound, ErrorKind::NotFound),
        (HostError::PermissionDenied, ErrorKind::PermissionDenied),
        (HostError::Failed, ErrorKind::Other),
    ];

    for (error, kind) in cases {
        // Act
        let result = hash(
            &api,
            foreign_source(PatternSource::failing(error)),
            checkpoint_into(&SpyStore::new()),
        )
        .await;

        // Assert
        assert_eq!(result, Err(SyncError::Io { kind }), "{error:?}");
    }
}

#[test]
fn an_unexpected_foreign_exception_is_a_host_failure() {
    // Arrange
    let thrown = uniffi::UnexpectedUniFFICallbackError::new("java.lang.IllegalStateException");

    // Act
    let error = HostError::from(thrown);

    // Assert
    assert_eq!(error, HostError::Failed);
}
