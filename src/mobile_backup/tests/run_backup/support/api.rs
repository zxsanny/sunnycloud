//! A fake of the sync-core boundary the loop drives (`BackupApi`). It does the real hashing and mirrors the
//! contract of `upload_small` (record saved before the first staging, results to the sink before the record is
//! cleared); the server it talks to is idempotent by key and commit id.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use mobile_backup::BackupApi;
use sc_clock::SystemClock;
use sync_core::Credential;
use sync_core::{
    BatchOutcome, BatchReport, BatchRunError, BatchSink, BoxFuture, CheckpointStore, FileSource,
    HashCheckpoint, HashedFile, ProgressSink, SkippedFile, SmallFile, SourceResolver, SyncError,
    UploadRequest, new_batch,
};
use sync_core_proto::{
    BLOCK_SIZE, CommitOutcome, FileHash, ListingPage, NodeKind, NodeView, PageToken, hash_block,
    rollup,
};
use tokio::sync::Notify;
use uuid::Uuid;

use super::db::Db;
pub use super::record::{CommitSeen, Gate, LargeCall, Record};
use super::server::{ROOT, Server};

type Hook = Box<dyn FnMut(usize) + Send>;

pub struct FakeApi {
    server: Arc<Server>,
    batch_max: usize,
    db: Mutex<Option<Db>>,
    record: Mutex<Record>,
    gate: Mutex<Option<Arc<Gate>>>,
    /// Like `gate`, but only `upload` and `upload_small` wait at it.
    upload_gate: Mutex<Option<Arc<Gate>>>,
    after_ack: Mutex<Option<Hook>>,
    on_call: Mutex<Option<Hook>>,
    /// Like the real client, cancelled for good: every later call ends `Cancelled` and sends nothing.
    cancelled: AtomicBool,
    cancel_signal: Notify,
    /// Errors by upload name (`IMG_<id>.jpg`).
    faults: Mutex<HashMap<String, SyncError>>,
    hash_faults: Mutex<HashMap<String, SyncError>>,
    quota_after: Mutex<Option<usize>>,
    /// The signed-in session the sync layer holds; the surface under test never sees it.
    credential: Mutex<Option<Credential>>,
}

impl FakeApi {
    pub fn new(server: Arc<Server>, batch_max: usize) -> Arc<Self> {
        Arc::new(Self {
            server,
            batch_max,
            db: Mutex::new(None),
            record: Mutex::default(),
            gate: Mutex::new(None),
            upload_gate: Mutex::new(None),
            after_ack: Mutex::new(None),
            on_call: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            cancel_signal: Notify::new(),
            faults: Mutex::default(),
            hash_faults: Mutex::default(),
            quota_after: Mutex::new(None),
            credential: Mutex::new(None),
        })
    }

    /// Every call reads this credential, as the real request path does.
    pub fn sign_in(&self, credential: Credential) {
        *self.credential.lock().expect("credential") = Some(credential);
    }

    /// The upload of the file with this name ends with `error`.
    pub fn fail_upload(&self, name: &str, error: SyncError) {
        self.faults
            .lock()
            .expect("faults")
            .insert(name.to_owned(), error);
    }

    pub fn fail_hash(&self, asset_id: i64, error: SyncError) {
        self.hash_faults
            .lock()
            .expect("hash faults")
            .insert(asset_id.to_string(), error);
    }

    pub fn clear_faults(&self) {
        self.faults.lock().expect("faults").clear();
        self.hash_faults.lock().expect("hash faults").clear();
        *self.quota_after.lock().expect("quota") = None;
    }

    /// Uploads end `QuotaExceeded` once this many assets are acknowledged.
    pub fn quota_after(&self, acknowledged: usize) {
        *self.quota_after.lock().expect("quota") = Some(acknowledged);
    }

    pub fn raise_quota(&self) {
        *self.quota_after.lock().expect("quota") = None;
    }

    /// Runs at the start of every call that reaches the server side, with the number of calls so far.
    pub fn on_call(&self, hook: impl FnMut(usize) + Send + 'static) {
        *self.on_call.lock().expect("hook") = Some(Box::new(hook));
    }

    fn quota_full(&self) -> bool {
        let limit = *self.quota_after.lock().expect("quota");
        limit.is_some_and(|limit| self.record().acked >= limit)
    }

    fn fault(&self, name: &str) -> Option<SyncError> {
        self.faults.lock().expect("faults").get(name).cloned()
    }

    pub fn record(&self) -> MutexGuard<'_, Record> {
        self.record.lock().expect("record")
    }

    /// Every upload then checks that its asset's hash and key were stored in this checkpoint first.
    pub fn watch(&self, db: Db) {
        *self.db.lock().expect("db") = Some(db);
    }

    pub fn close_gate(&self) -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        *self.gate.lock().expect("gate") = Some(Arc::clone(&gate));
        gate
    }

    pub fn hold_uploads(&self) -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        *self.upload_gate.lock().expect("gate") = Some(Arc::clone(&gate));
        gate
    }

    pub fn open_gate(&self, gate: &Gate) {
        *self.gate.lock().expect("gate") = None;
        gate.release.notify_one();
    }

    /// Runs after the server acknowledged a commit, with the number of acknowledged assets so far.
    pub fn on_ack(&self, hook: impl FnMut(usize) + Send + 'static) {
        *self.after_ack.lock().expect("hook") = Some(Box::new(hook));
    }

    async fn enter(&self) -> Result<(), SyncError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(SyncError::Cancelled);
        }
        let signed_in = self.credential.lock().expect("credential").is_some();
        let calls = {
            let mut record = self.record();
            record.calls += 1;
            record.bearer_uses += usize::from(signed_in);
            record.calls
        };
        if let Some(hook) = self.on_call.lock().expect("hook").as_mut() {
            hook(calls);
        }
        let gate = self.gate.lock().expect("gate").clone();
        self.wait_at(gate).await?;
        tokio::task::yield_now().await;
        Ok(())
    }

    /// An upload is about to be sent: it waits here when the test holds uploads.
    async fn hold_upload(&self) -> Result<(), SyncError> {
        let gate = self.upload_gate.lock().expect("gate").clone();
        self.wait_at(gate).await
    }

    async fn wait_at(&self, gate: Option<Arc<Gate>>) -> Result<(), SyncError> {
        if let Some(gate) = gate {
            gate.entered.notify_one();
            tokio::select! {
                () = gate.release.notified() => {}
                () = self.cancel_signal.notified() => return Err(SyncError::Cancelled),
            }
        }
        Ok(())
    }

    /// An upload is starting: its assets must have their hash stored, and the rows holding a stored hash that
    /// is not yet uploaded are the window the loop keeps hashed.
    fn started(&self, keys: &[Uuid]) {
        let (unstored, unfinished) = match self.db.lock().expect("db").as_ref() {
            Some(db) => (
                keys.iter()
                    .filter(|key| {
                        !db.asset_of_key(**key).is_some_and(|row| {
                            row.state == "uploading" && row.content_hash.is_some()
                        })
                    })
                    .copied()
                    .collect(),
                db.count("uploading"),
            ),
            None => (Vec::new(), 0),
        };
        let mut record = self.record();
        record.unstored.extend(unstored);
        record.most_unfinished = record.most_unfinished.max(unfinished);
    }

    fn acknowledged(&self, count: usize) {
        let total = {
            let mut record = self.record();
            record.acked += count;
            record.acked
        };
        if let Some(hook) = self.after_ack.lock().expect("hook").as_mut() {
            hook(total);
        }
    }
}

pub fn hash_of(bytes: &[u8]) -> FileHash {
    let hashes: Vec<_> = bytes
        .chunks(BLOCK_SIZE)
        .map(|chunk| hash_block(chunk).expect("a block"))
        .collect();
    rollup(&hashes)
}

fn read_all(source: &dyn FileSource) -> Vec<u8> {
    let len = source.len().expect("len");
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < len {
        let want = (len - bytes.len() as u64).min(BLOCK_SIZE as u64) as u32;
        bytes.extend_from_slice(&source.read_at(bytes.len() as u64, want).expect("read"));
    }
    bytes
}

impl BackupApi for FakeApi {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_signal.notify_waiters();
        self.cancel_signal.notify_one();
    }

    fn batch_commit_max(&self) -> usize {
        self.batch_max
    }

    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>> {
        Box::pin(async move {
            self.enter().await?;
            Ok(NodeView {
                node_id: ROOT,
                parent_id: None,
                kind: NodeKind::Folder,
                name: String::new(),
                current_revision: None,
            })
        })
    }

    fn list_children<'a>(
        &'a self,
        node: Uuid,
        _after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>> {
        Box::pin(async move {
            self.enter().await?;
            self.record().lists += 1;
            Ok(self.server.children(node))
        })
    }

    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        Box::pin(async move {
            self.enter().await?;
            self.record().creates += 1;
            self.server.create_folder(parent, name)
        })
    }

    fn hash_file<'a>(
        &'a self,
        source: Arc<dyn FileSource>,
        _progress: Arc<dyn ProgressSink>,
        checkpoint: HashCheckpoint,
    ) -> BoxFuture<'a, Result<HashedFile, SyncError>> {
        Box::pin(async move {
            self.enter().await?;
            if let Some(error) = self
                .hash_faults
                .lock()
                .expect("hash faults")
                .get(&checkpoint.local_key)
                .cloned()
            {
                return Err(error);
            }
            let bytes = read_all(source.as_ref());
            self.record().hashed.push(checkpoint.local_key);
            Ok(HashedFile {
                block_hashes: bytes
                    .chunks(BLOCK_SIZE)
                    .map(|chunk| hash_block(chunk).expect("a block"))
                    .collect(),
                file_hash: hash_of(&bytes),
            })
        })
    }

    fn upload(
        &self,
        request: UploadRequest,
        checkpoints: Arc<dyn CheckpointStore>,
        _progress: Arc<dyn ProgressSink>,
    ) -> BoxFuture<'_, Result<CommitOutcome, SyncError>> {
        Box::pin(async move {
            self.enter().await?;
            self.hold_upload().await?;
            if self.quota_full() {
                return Err(SyncError::QuotaExceeded);
            }
            self.record()
                .attempts
                .push((request.name.clone(), request.parent));
            if let Some(error) = self.fault(&request.name) {
                return Err(error);
            }
            if !self.server.state().has_folder(request.parent) {
                return Err(self.server.missing_parent_error());
            }
            self.started(&[request.client_key]);
            let session = checkpoints.load(&request.local_key).expect("session row");
            let size = request.source.len().expect("len");
            let file =
                self.server
                    .new_file(request.parent, &request.name, request.hashed.file_hash);
            let outcome = self.server.commit_file(request.client_key, file);
            let bytes_sent = match outcome {
                CommitOutcome::NothingToSend { .. } => 0,
                _ => size,
            };
            self.record().large.push(LargeCall {
                local_key: request.local_key.clone(),
                key: request.client_key,
                name: request.name,
                parent: request.parent,
                meta: request.client_meta,
                session,
                bytes_sent,
            });
            self.acknowledged(1);
            checkpoints
                .clear(&request.local_key)
                .expect("clear session");
            Ok(outcome)
        })
    }

    fn upload_small(
        &self,
        files: Vec<SmallFile>,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
    ) -> BoxFuture<'_, Result<Vec<SkippedFile>, BatchRunError>> {
        Box::pin(async move {
            self.enter().await?;
            self.record().groups.push(files.len());
            self.hold_upload().await?;
            if self.quota_full() {
                return Err(SyncError::QuotaExceeded.into());
            }
            let mut left_out = Vec::new();
            let mut kept = Vec::new();
            for file in files {
                self.record()
                    .attempts
                    .push((file.entry.name.clone(), file.entry.parent));
                match self.fault(&file.entry.name) {
                    Some(
                        error @ (SyncError::SourceChanged
                        | SyncError::Io { .. }
                        | SyncError::SizeTooLarge),
                    ) => left_out.push(SkippedFile {
                        entry: file.entry,
                        error,
                    }),
                    Some(error) => return Err(error.into()),
                    None => kept.push(file),
                }
            }
            for chunk in kept.chunks(self.batch_max) {
                let entries: Vec<_> = chunk.iter().map(|file| file.entry.clone()).collect();
                let keys: Vec<Uuid> = entries.iter().map(|entry| entry.staged_key).collect();
                self.started(&keys);
                let batch = new_batch(&SystemClock::new(), entries.clone()).expect("batch ids");
                store.save_batch(batch.clone())?;
                let stored = store
                    .load_batches()?
                    .iter()
                    .any(|record| record.commit_id == batch.commit_id);
                for file in chunk {
                    let bytes = read_all(file.source.as_ref());
                    assert_eq!(
                        hash_of(&bytes),
                        file.entry.file_hash,
                        "staged bytes match their hash"
                    );
                    tokio::task::yield_now().await;
                }
                let result = self.server.commit_batch(batch.commit_id, &entries);
                self.record().commits.push(CommitSeen {
                    commit_id: batch.commit_id,
                    entries: Vec::new(),
                    stored_before_staging: stored,
                });
                self.record().commits.last_mut().expect("commit").entries = entries
                    .iter()
                    .map(|entry| (entry.staged_key, entry.parent, entry.client_meta.clone()))
                    .collect();
                self.acknowledged(entries.len());
                sink.on_batch(BatchReport {
                    commit_id: batch.commit_id,
                    entries,
                    outcome: BatchOutcome::Committed(result),
                })?;
                store.clear_batch(batch.commit_id)?;
            }
            Ok(left_out)
        })
    }

    fn resume_batches<'a>(
        &'a self,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
        resolve: SourceResolver<'a>,
    ) -> BoxFuture<'a, Result<Vec<SkippedFile>, BatchRunError>> {
        Box::pin(async move {
            self.enter().await?;
            for record in store.load_batches()? {
                for entry in &record.entries {
                    assert!(
                        resolve(entry).is_some(),
                        "the loop finds the source of a staged entry"
                    );
                }
                self.record().resumed.push(record.commit_id);
                let result = self.server.commit_batch(record.commit_id, &record.entries);
                sink.on_batch(BatchReport {
                    commit_id: record.commit_id,
                    entries: record.entries,
                    outcome: BatchOutcome::Committed(result),
                })?;
                store.clear_batch(record.commit_id)?;
            }
            Ok(Vec::new())
        })
    }
}
