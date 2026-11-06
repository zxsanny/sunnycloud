//! The batch executor: save the record, stage the members in parallel, commit, hand the results to the host,
//! clear the record. All state is in the host's store; a dropped run is continued by `resume`.

use std::collections::VecDeque;
use std::sync::Arc;

use sc_clock::Clock;
use sync_core_proto::{BatchCommit, BatchEntry};
use tokio::task::JoinSet;
use uuid::Uuid;

use super::member::{Member, blocking, joined, stage_member};
use super::plan::chunk_len;
use super::wire::batch_commit;
use super::{
    BatchOutcome, BatchReport, BatchRunError, BatchSink, SMALL_FILE_MAX, SkippedFile, SmallFile,
    SourceResolver, new_batch,
};
use crate::error::SyncError;
use crate::host::{BatchRecord, CheckpointStore, StoreError};
use crate::negotiate::Limits;
use crate::request::RequestPath;

pub(crate) struct Runner {
    path: Arc<RequestPath>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn CheckpointStore>,
    sink: Arc<dyn BatchSink>,
    /// `min(batch_max_entries, max_staged_per_account / 2)`: one batch is all this client has staged and uncommitted.
    max_entries: usize,
    max_bytes: u64,
    /// Bodies in flight: `batch_max_bytes_in_flight` over the largest body.
    lanes: usize,
}

struct Staging {
    staged: Vec<Member>,
    /// Refused with `StagedCapReached`: staged again after this batch is committed.
    deferred: Vec<Member>,
}

impl Runner {
    pub(crate) fn new(
        path: Arc<RequestPath>,
        limits: Limits,
        max_bytes: u64,
        clock: Arc<dyn Clock>,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
    ) -> Self {
        let half_cap = (limits.max_staged_per_account / 2).max(1);
        Self {
            path,
            clock,
            store,
            sink,
            max_entries: usize::try_from(limits.batch_max_entries.min(half_cap))
                .unwrap_or(usize::MAX),
            max_bytes,
            lanes: usize::try_from(max_bytes / SMALL_FILE_MAX)
                .unwrap_or(usize::MAX)
                .max(1),
        }
    }

    pub(crate) async fn upload(
        &self,
        files: Vec<SmallFile>,
    ) -> Result<Vec<SkippedFile>, BatchRunError> {
        let mut skipped = Vec::new();
        let pending = self.measure(files, &mut skipped).await?;
        self.drain(pending, &mut skipped).await?;
        Ok(skipped)
    }

    /// Replays every record of the store with its own id and entries. Members the server lost are staged
    /// again from their sources before the commit. Members the server refuses for the staged cap go to
    /// new batches after the replayed ones.
    pub(crate) async fn resume(
        &self,
        resolve: SourceResolver<'_>,
    ) -> Result<Vec<SkippedFile>, BatchRunError> {
        let records = self.store(|store| store.load_batches()).await?;
        let mut skipped = Vec::new();
        let mut deferred = VecDeque::new();
        for record in records {
            self.path.check()?;
            let record_len = record.entries.len();
            let mut files = Vec::with_capacity(record_len);
            for entry in record.entries {
                match resolve(&entry) {
                    Some(source) => files.push(SmallFile { entry, source }),
                    None => skipped.push(SkippedFile {
                        entry,
                        error: SyncError::Io {
                            kind: std::io::ErrorKind::NotFound,
                        },
                    }),
                }
            }
            let members = Vec::from(self.measure(files, &mut skipped).await?);
            let refused = self
                .run_batch(record.commit_id, record_len, members, &mut skipped)
                .await?;
            deferred.extend(refused);
        }
        self.drain(deferred, &mut skipped).await?;
        Ok(skipped)
    }

    async fn measure(
        &self,
        files: Vec<SmallFile>,
        skipped: &mut Vec<SkippedFile>,
    ) -> Result<VecDeque<Member>, SyncError> {
        let (members, left_out) = blocking(move || {
            let (mut members, mut left_out) = (VecDeque::with_capacity(files.len()), Vec::new());
            for file in files {
                match file.source.len() {
                    Ok(size) => members.push_back(Member { file, size }),
                    Err(error) => left_out.push(SkippedFile {
                        entry: file.entry,
                        error: SyncError::Io { kind: error.kind() },
                    }),
                }
            }
            (members, left_out)
        })
        .await?;
        for file in left_out {
            note_skipped(skipped, file);
        }
        Ok(members)
    }

    async fn drain(
        &self,
        mut pending: VecDeque<Member>,
        skipped: &mut Vec<SkippedFile>,
    ) -> Result<(), BatchRunError> {
        while !pending.is_empty() {
            self.path.check()?;
            let count = chunk_len(
                pending.iter().map(|member| member.size),
                self.max_entries,
                self.max_bytes,
            );
            let chunk: Vec<Member> = pending.drain(..count).collect();
            let entries = chunk.iter().map(|member| member.file.entry.clone());
            let record = new_batch(self.clock.as_ref(), entries.collect()).map_err(|_| {
                SyncError::ProtocolViolation {
                    detail: "batch commit id equals a member key",
                }
            })?;
            let commit_id = record.commit_id;
            self.store(move |store| store.save_batch(record)).await?;
            let deferred = self.run_batch(commit_id, count, chunk, skipped).await?;
            for member in deferred.into_iter().rev() {
                pending.push_front(member);
            }
        }
        Ok(())
    }

    /// `record_len` is the number of entries the saved record holds. Returns the members to stage again later.
    async fn run_batch(
        &self,
        commit_id: Uuid,
        record_len: usize,
        members: Vec<Member>,
        skipped: &mut Vec<SkippedFile>,
    ) -> Result<Vec<Member>, BatchRunError> {
        let Staging { staged, deferred } = self.stage_all(members, skipped).await?;
        if staged.is_empty() {
            self.store(move |store| store.clear_batch(commit_id))
                .await?;
            return if deferred.is_empty() {
                Ok(deferred)
            } else {
                Err(SyncError::StagedCapReached.into())
            };
        }
        let entries: Vec<BatchEntry> = staged
            .iter()
            .map(|member| member.file.entry.clone())
            .collect();
        if entries.len() < record_len {
            let shrunk = BatchRecord {
                commit_id,
                entries: entries.clone(),
            };
            self.store(move |store| store.save_batch(shrunk)).await?;
        }
        let commit = BatchCommit { commit_id, entries };
        let outcome = match batch_commit(&self.path, &commit).await {
            Ok(result) if result.entries.len() == commit.entries.len() => {
                BatchOutcome::Committed(result)
            }
            Ok(_) => {
                return Err(SyncError::ProtocolViolation {
                    detail: "batch result does not match its request",
                }
                .into());
            }
            Err(SyncError::CommitIdReused) => BatchOutcome::IdReused,
            Err(error) => return Err(error.into()),
        };
        let report = BatchReport {
            commit_id,
            entries: commit.entries,
            outcome,
        };
        let sink = Arc::clone(&self.sink);
        blocking(move || sink.on_batch(report)).await??;
        self.store(move |store| store.clear_batch(commit_id))
            .await?;
        Ok(deferred)
    }

    /// Stages the members over `lanes` parallel requests. After the first `StagedCapReached` nothing new is
    /// started: the batch commits what is staged.
    async fn stage_all(
        &self,
        members: Vec<Member>,
        skipped: &mut Vec<SkippedFile>,
    ) -> Result<Staging, BatchRunError> {
        let mut queue = members.into_iter().enumerate();
        let mut running = JoinSet::new();
        let mut staged = Vec::new();
        let mut deferred = Vec::new();
        let mut capped = false;
        loop {
            while !capped
                && running.len() < self.lanes
                && let Some((index, member)) = queue.next()
            {
                let path = Arc::clone(&self.path);
                running.spawn(async move {
                    let result = stage_member(&path, &member).await;
                    (index, member, result)
                });
            }
            let Some(finished) = running.join_next().await else {
                break;
            };
            let (index, member, result) = joined(finished)?;
            match result {
                Ok(()) => staged.push((index, member)),
                Err(SyncError::StagedCapReached) => {
                    capped = true;
                    deferred.push((index, member));
                }
                Err(
                    error @ (SyncError::SourceChanged
                    | SyncError::Io { .. }
                    | SyncError::SizeTooLarge),
                ) => note_skipped(
                    skipped,
                    SkippedFile {
                        entry: member.file.entry,
                        error,
                    },
                ),
                Err(error) => return Err(error.into()),
            }
        }
        deferred.extend(queue);
        staged.sort_by_key(|(index, _)| *index);
        deferred.sort_by_key(|(index, _)| *index);
        Ok(Staging {
            staged: staged.into_iter().map(|(_, member)| member).collect(),
            deferred: deferred.into_iter().map(|(_, member)| member).collect(),
        })
    }

    async fn store<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn CheckpointStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, BatchRunError> {
        let store = Arc::clone(&self.store);
        Ok(blocking(move || work(store.as_ref())).await??)
    }
}

fn note_skipped(skipped: &mut Vec<SkippedFile>, left_out: SkippedFile) {
    tracing::warn!(error = %left_out.error, "small file left out of its batch");
    skipped.push(left_out);
}
