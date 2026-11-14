//! `commit_batch` (`commit_service.md`): up to `batch_max_entries` staged files become revisions through one
//! `TreeCommit::publish` call under a client commit id; 03 never opens the commit transaction (ADR-015).
//!
//! Outside any transaction: the size limit, the request hash, the staged rows the entries name (one short
//! transaction behind the account guard), and per entry the blob's key material, the media type sniffed from at
//! most 64 KiB of its head and the target, chosen as by `commit_upload`. An entry without an open staged row (or
//! whose uploading blob is gone, which a concurrent identical batch may have committed) is refused there with
//! `UploadNotFound`; the receipt stores it like any other outcome. `publish` then runs [`ReceiptEffect`] (replay
//! or `CommitIdReused` under the account lock), [`StagedCommitEffect`] and the injected effects. After COMMIT
//! the unused files of `NoChange` entries are discarded.

use std::collections::HashMap;
use std::sync::Arc;

use contracts::{
    BatchCommit, BatchEntry, BatchResult, BlobError, CommitActor, CommitEffect, CommitServiceError,
    EntryError, EntryResult, NewRevision, PublishEntry, PublishOutcome, PublishResult, Target,
    UploadError,
};
use sc_db::Scope;
use sc_ids::{FileHash, Hash32, rollup};
use sc_observability::log_error;
use uuid::Uuid;

use crate::batch_effect::{ReceiptEffect, StagedCommitEffect, StagedPlan};
use crate::batch_response::{assemble, decode, request_hash};
use crate::commit::{publish_error, unexpected};
use crate::session_repo::{self, StagedRow};
use crate::sniff::{HEAD_LEN, sniff};
use crate::upload::PgUploadService;
use crate::upload_metrics::{CommitKind, CommitResult};

impl PgUploadService {
    pub async fn commit_batch(
        &self,
        actor: CommitActor,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        let began = self.clock.monotonic();
        let result = self.commit_many(actor, request).await;
        if let Some(metrics) = &self.metrics {
            let elapsed = self.clock.monotonic().saturating_duration_since(began);
            let outcome = match &result {
                Ok(_) => CommitResult::Committed,
                Err(CommitServiceError::Unexpected(_)) => CommitResult::Failed,
                Err(_) => CommitResult::Refused,
            };
            metrics.commit(CommitKind::Batch, outcome, elapsed);
        }
        result
    }

    async fn commit_many(
        &self,
        actor: CommitActor,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        if u64::try_from(request.entries.len()).unwrap_or(u64::MAX) > self.config.batch_max_entries
        {
            return Err(CommitServiceError::BatchTooLarge);
        }
        let account = actor.account_id;
        let request_hash = request_hash(&request.entries);
        let mut rows = self.staged_rows(account, &request.entries).await?;
        let mut refused = Vec::with_capacity(request.entries.len());
        let mut entries = Vec::new();
        let mut plans = Vec::new();
        for entry in request.entries {
            let row = rows.remove(&entry.staged_key.to_string());
            match self.prepare(account, entry, row).await? {
                Some((publish, plan)) => {
                    refused.push(None);
                    entries.push(publish);
                    plans.push(plan);
                }
                None => refused.push(Some(EntryError::UploadNotFound)),
            }
        }
        let blobs: Vec<Uuid> = plans.iter().map(|plan| plan.blob_id).collect();
        let now = self.clock.now_utc();
        let receipt = ReceiptEffect {
            account,
            commit_id: request.commit_id,
            request_hash,
            refused: refused.clone(),
            now,
        };
        let staged = StagedCommitEffect::new(
            Arc::clone(&self.registry),
            account,
            plans,
            self.config.max_vault_file_bytes,
        );
        let mut effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(receipt), Arc::new(staged)];
        effects.extend(self.commit_effects.iter().cloned());
        let published = self
            .commit
            .publish(actor, entries, effects, now)
            .await
            .map_err(publish_error)?;
        match published {
            PublishResult::Replayed(stored) => decode(&stored.0).map_err(unexpected),
            PublishResult::Published(results) => {
                self.discard_unchanged(account, &results, &blobs).await;
                assemble(&refused, results.iter()).map_err(unexpected)
            }
        }
    }

    /// The open staged rows the entries name, by key, read behind the account guard. A key named twice is one
    /// row: the first entry takes it and the second is refused with `UploadNotFound`.
    async fn staged_rows(
        &self,
        account: Uuid,
        entries: &[BatchEntry],
    ) -> Result<HashMap<String, StagedRow>, CommitServiceError> {
        let keys: Vec<String> = entries
            .iter()
            .map(|entry| entry.staged_key.to_string())
            .collect();
        let now = self.clock.now_utc();
        let rows = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await?;
                session_repo::staged_by_keys(tx, account, &keys, now)
                    .await
                    .map_err(crate::upload::unexpected)
            })
            .await
            .map_err(crate::upload::run_error)
            .map_err(|error| match error {
                UploadError::AccountNotActive => CommitServiceError::AccountNotActive,
                other => unexpected(other),
            })?;
        Ok(rows
            .into_iter()
            .map(|row| (row.client_key.clone(), row))
            .collect())
    }

    /// The `publish` entry of one request entry (its staged row, sniffed head and target); `None` when its staged
    /// row or uploading blob is gone (`UploadNotFound`).
    async fn prepare(
        &self,
        account: Uuid,
        entry: BatchEntry,
        row: Option<StagedRow>,
    ) -> Result<Option<(PublishEntry, StagedPlan)>, CommitServiceError> {
        let Some(row) = row else {
            return Ok(None);
        };
        let Some(plan) = self
            .catalog
            .get_uploading_plan(account, row.blob_id)
            .await
            .map_err(unexpected)?
        else {
            return Ok(None);
        };
        let head = match self
            .reader
            .read_head(
                row.blob_id,
                &plan.wrapped_key,
                plan.master_key_id,
                row.declared_size,
                HEAD_LEN,
            )
            .await
        {
            Ok(head) => head,
            Err(BlobError::BlobMissing) => return Ok(None),
            Err(other) => return Err(unexpected(other)),
        };
        let target = self
            .target(account, entry.parent_id, &entry.name, entry.base_revision)
            .await?;
        let blocklist = match <[u8; 32]>::try_from(row.first_hash.as_slice()) {
            Ok(bytes) => vec![Hash32::from_bytes(bytes)],
            Err(_) => Vec::new(),
        };
        let content_hash: FileHash = rollup(&blocklist);
        let revision_id = sc_ids::new_id(self.clock.as_ref());
        let existing_node = match &target {
            Target::Existing { node_id, .. } => Some(*node_id),
            Target::New { .. } => None,
        };
        let publish = PublishEntry {
            target,
            revision: NewRevision {
                revision_id,
                blob_id: row.blob_id,
                size: row.declared_size,
                content_hash,
                blocklist,
                media_type: sniff(&head).map(str::to_owned),
                client_mtime: entry.client_mtime,
                client_meta: entry.client_meta,
                reserved_bytes: row.reserved_bytes,
                import_hint: entry.import_hint,
            },
            vault: false,
        };
        let plan = StagedPlan {
            session_id: row.id,
            blob_id: row.blob_id,
            size: row.declared_size,
            revision_id,
            existing_node,
            content_hash,
        };
        Ok(Some((publish, plan)))
    }

    /// Best effort after COMMIT: the file of a `NoChange` entry has no row any more; a file left behind is the
    /// orphan sweeper's (04).
    async fn discard_unchanged(&self, account: Uuid, results: &[EntryResult], blobs: &[Uuid]) {
        for (result, blob_id) in results.iter().zip(blobs) {
            let EntryResult::Published {
                outcome: PublishOutcome::NoChange { .. },
                ..
            } = result
            else {
                continue;
            };
            if let Err(error) = self.writer.discard(*blob_id).await {
                log_error!(
                    "unused staged file not discarded",
                    account_id = account,
                    err = &error
                );
            }
        }
    }
}
