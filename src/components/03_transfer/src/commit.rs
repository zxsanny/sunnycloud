//! `commit_upload` (`commit_service.md` 1.2.0): one large upload becomes a revision through one
//! `TreeCommit::publish` call; 03 never opens the commit transaction (ADR-015).
//!
//! Outside any transaction, in time independent of the file size (AC-4): the session's progress entry (loaded from
//! its checkpoint row when this process has none), the blocklist against the received hashes, the file hash as
//! the roll-up of the 32-byte block hashes, and the media type sniffed from at most 64 KiB of the head (the only
//! file bytes read). Then the target: `Existing` on the node of `base_revision`, else `New{parent_id, name}`;
//! `publish` runs [`BlobCommitEffect`] and the injected effects inside 02's transaction. After a commit the
//! progress entry is evicted; after `NoChange` the unused upload file is discarded.

use std::sync::Arc;

use contracts::{
    CommitActor, CommitEffect, CommitServiceError, CommitUpload, Committed, EntryFailure,
    EntryResult, NewRevision, PublishEntry, PublishOutcome, PublishResult, Target, TreeCommitError,
    TreeReaderError, UploadError,
};
use sc_ids::{FileHash, Hash32, rollup};
use sc_observability::log_error;
use thiserror::Error;
use uuid::Uuid;

use crate::commit_effect::{BlobCommitEffect, SESSION_GONE};
use crate::progress::{SessionProgress, SharedProgress};
use crate::sniff::{HEAD_LEN, sniff};
use crate::upload::PgUploadService;
use crate::upload_metrics::{CommitResult, SessionResult};

/// Unexpected outcomes of a commit: each means 02 or 03 broke its own contract, or a purge raced the commit.
#[derive(Debug, Error)]
enum CommitFault {
    #[error("publish replayed a stored response for a single commit")]
    Replayed,
    #[error("publish returned {0} results for one entry")]
    ResultCount(usize),
    #[error("the node of the base revision is gone")]
    TargetGone,
    #[error("an effect failed the entry: {}", .0.as_str())]
    Effect(sc_error::ErrorCode),
    #[error("record expiry overflows the timestamp range")]
    ExpiryOverflow,
}

/// What the checks outside the transaction found.
struct Checked {
    file_hash: FileHash,
    blocklist: Vec<Hash32>,
    media_type: Option<&'static str>,
}

impl PgUploadService {
    pub async fn commit_upload(
        &self,
        actor: CommitActor,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        let began = self.clock.monotonic();
        let result = self.commit_one(actor, request).await;
        if let Some(metrics) = &self.metrics {
            let elapsed = self.clock.monotonic().saturating_duration_since(began);
            let outcome = match &result {
                Ok(_) => CommitResult::Committed,
                Err(CommitServiceError::Unexpected(_)) => CommitResult::Failed,
                Err(_) => CommitResult::Refused,
            };
            metrics.block_list_commit(outcome, elapsed);
        }
        result
    }

    async fn commit_one(
        &self,
        actor: CommitActor,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        let account = actor.account_id;
        let upload_id = request.upload_id;
        let session = self
            .live_session(account, upload_id)
            .await
            .map_err(session_error)?;
        let checked = self.check(&session, &request.blocklist).await?;
        let target = self.target(account, &request).await?;
        let now = self.clock.now_utc();
        let info = session.info();
        let revision_id = sc_ids::new_id(self.clock.as_ref());
        let effect = BlobCommitEffect {
            registry: Arc::clone(&self.registry),
            account,
            upload_id,
            blob_id: info.handle.plan.blob_id,
            size: info.declared_size,
            revision_id,
            record_expires_at: now
                .checked_add(self.config.upload_session_ttl)
                .ok_or_else(|| unexpected(CommitFault::ExpiryOverflow))?,
        };
        let entry = PublishEntry {
            target,
            revision: NewRevision {
                revision_id,
                blob_id: info.handle.plan.blob_id,
                size: info.declared_size,
                content_hash: checked.file_hash,
                blocklist: checked.blocklist,
                media_type: checked.media_type.map(str::to_owned),
                client_mtime: request.client_mtime,
                client_meta: request.client_meta,
                // A large session reserves exactly its declared size (`session_repo::insert`) until its commit.
                reserved_bytes: info.declared_size,
                import_hint: request.import_hint,
            },
            vault: false,
        };
        let mut effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(effect)];
        effects.extend(self.commit_effects.iter().cloned());
        let published = self
            .commit
            .publish(actor, vec![entry], effects, now)
            .await
            .map_err(publish_error)?;
        let result = single(published)?;
        self.settled(account, upload_id, info.handle.plan.blob_id, result)
            .await
    }

    /// The blocklist against the received hashes, the roll-up and the sniffed head.
    async fn check(
        &self,
        session: &SharedProgress,
        blocklist: &[Hash32],
    ) -> Result<Checked, CommitServiceError> {
        let hashes = check_blocklist(&session.lock(), blocklist)?;
        let file_hash = rollup(&hashes);
        let handle = &session.info().handle;
        let head = self
            .reader
            .read_head(
                handle.plan.blob_id,
                &handle.plan.wrapped_key,
                handle.plan.master_key_id,
                handle.plaintext_size,
                HEAD_LEN,
            )
            .await
            .map_err(unexpected)?;
        Ok(Checked {
            file_hash,
            blocklist: hashes,
            media_type: sniff(&head),
        })
    }

    /// `Existing` on the node of `base_revision`; without one, or when the account has no such revision (an
    /// unknown or foreign base is ignored, as by `start_upload`), `New{parent_id, name}`.
    async fn target(
        &self,
        account: Uuid,
        request: &CommitUpload,
    ) -> Result<Target, CommitServiceError> {
        let new = || Target::New {
            parent_id: request.parent_id,
            name: request.name.clone(),
        };
        let Some(base) = request.base_revision else {
            return Ok(new());
        };
        match self.tree.get_revision(account, base).await {
            Ok(revision) => Ok(Target::Existing {
                node_id: revision.node_id,
                base_revision: Some(base),
            }),
            Err(TreeReaderError::NotFound) => Ok(new()),
            Err(other) => Err(unexpected(other)),
        }
    }

    /// Evicts the session after the commit (or when its row was gone) and maps the entry result.
    async fn settled(
        &self,
        account: Uuid,
        upload_id: Uuid,
        blob_id: Uuid,
        result: EntryResult,
    ) -> Result<Committed, CommitServiceError> {
        match result {
            EntryResult::Published {
                outcome,
                node_id,
                revision_id,
                conflict_copy,
            } => {
                self.evict(upload_id);
                if let Some(metrics) = &self.metrics {
                    metrics.session_ended(SessionResult::Committed);
                }
                if matches!(outcome, PublishOutcome::NoChange { .. }) {
                    self.discard_unused(account, blob_id).await;
                }
                Ok(Committed {
                    node_id,
                    revision_id,
                    conflict_copy,
                })
            }
            // 03 publishes the blob of its own session, so a failed ownership check means the blob row went with
            // the session (abort and the reaper remove both in one transaction) before 02 wrote the revision.
            EntryResult::Failed(
                EntryFailure::EffectFailed(SESSION_GONE) | EntryFailure::BlocklistMismatch,
            ) => {
                self.evict(upload_id);
                Err(CommitServiceError::UploadNotFound)
            }
            EntryResult::Failed(failure) => Err(entry_error(failure)),
        }
    }

    /// Best effort after COMMIT: a file left behind has no row and is the orphan sweeper's (04).
    async fn discard_unused(&self, account: Uuid, blob_id: Uuid) {
        if let Err(error) = self.writer.discard(blob_id).await {
            log_error!(
                "unused upload file not discarded",
                account_id = account,
                err = &error
            );
        }
    }
}

/// The ordered block hashes when `blocklist` matches the session: a count other than the session's block count or
/// a differing hash is `BlocklistMismatch`; blocks not yet received are `NeedBlocks`.
pub(crate) fn check_blocklist(
    progress: &SessionProgress,
    blocklist: &[Hash32],
) -> Result<Vec<Hash32>, CommitServiceError> {
    if blocklist.len() != progress.block_count() as usize {
        return Err(CommitServiceError::BlocklistMismatch);
    }
    let missing = progress.missing_blocks();
    if !missing.is_empty() {
        return Err(CommitServiceError::NeedBlocks(missing));
    }
    let mut hashes = Vec::with_capacity(blocklist.len());
    for (index, sent) in (0..progress.block_count()).zip(blocklist) {
        match progress.hash(index) {
            Ok(Some(stored)) if stored == *sent => hashes.push(stored),
            _ => return Err(CommitServiceError::BlocklistMismatch),
        }
    }
    Ok(hashes)
}

fn single(published: PublishResult) -> Result<EntryResult, CommitServiceError> {
    match published {
        PublishResult::Published(mut results) if results.len() == 1 => {
            Ok(results.pop().expect("one result"))
        }
        PublishResult::Published(results) => {
            Err(unexpected(CommitFault::ResultCount(results.len())))
        }
        PublishResult::Replayed(_) => Err(unexpected(CommitFault::Replayed)),
    }
}

fn entry_error(failure: EntryFailure) -> CommitServiceError {
    match failure {
        EntryFailure::NameInvalid(_) => CommitServiceError::InvalidName,
        EntryFailure::ParentDeleted => CommitServiceError::ParentGone,
        EntryFailure::ParentNotFound => CommitServiceError::ParentNotFound,
        EntryFailure::NotAFolder => CommitServiceError::NotAFolder,
        EntryFailure::BlocklistMismatch => CommitServiceError::UploadNotFound,
        EntryFailure::NotFound => unexpected(CommitFault::TargetGone),
        EntryFailure::EffectFailed(code) => unexpected(CommitFault::Effect(code)),
    }
}

fn publish_error(error: TreeCommitError) -> CommitServiceError {
    match error {
        TreeCommitError::AccountNotActive => CommitServiceError::AccountNotActive,
        TreeCommitError::Unexpected(fault) => CommitServiceError::Unexpected(fault),
        other => unexpected(other),
    }
}

fn session_error(error: UploadError) -> CommitServiceError {
    match error {
        UploadError::UploadNotFound => CommitServiceError::UploadNotFound,
        UploadError::Unexpected(fault) => CommitServiceError::Unexpected(fault),
        other => unexpected(other),
    }
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> CommitServiceError {
    CommitServiceError::Unexpected(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> Hash32 {
        Hash32::from_bytes([byte; 32])
    }

    /// Five blocks, each with the hash `[index + 1; 32]`, received except `missing`.
    fn progress(missing: &[u32]) -> SessionProgress {
        let mut progress = SessionProgress::new(5);
        for index in (0..5).filter(|index| !missing.contains(index)) {
            let byte = u8::try_from(index + 1).expect("small index");
            progress
                .mark_received(index, hash(byte))
                .expect("block in range");
        }
        progress
    }

    fn full_list() -> Vec<Hash32> {
        (1..=5).map(hash).collect()
    }

    #[test]
    fn a_complete_matching_blocklist_returns_the_hashes_in_order() {
        // Arrange
        let progress = progress(&[]);

        // Act
        let hashes = check_blocklist(&progress, &full_list());

        // Assert
        assert_eq!(hashes.expect("matching blocklist"), full_list());
    }

    #[test]
    fn a_missing_block_is_needed_again() {
        // Arrange
        let progress = progress(&[2]);

        // Act
        let result = check_blocklist(&progress, &full_list());

        // Assert
        assert!(
            matches!(result, Err(CommitServiceError::NeedBlocks(ref need)) if need == &[2]),
            "{result:?}"
        );
    }

    #[test]
    fn a_wrong_hash_is_a_mismatch() {
        // Arrange
        let progress = progress(&[]);
        let mut list = full_list();
        list[3] = hash(0xee);

        // Act
        let result = check_blocklist(&progress, &list);

        // Assert
        assert!(
            matches!(result, Err(CommitServiceError::BlocklistMismatch)),
            "{result:?}"
        );
    }

    #[test]
    fn a_wrong_count_is_a_mismatch_even_with_blocks_missing() {
        // Arrange
        let complete = progress(&[]);
        let partial = progress(&[4]);
        let short = full_list()[..3].to_vec();

        // Act
        let results = [
            check_blocklist(&complete, &short),
            check_blocklist(&partial, &short),
        ];

        // Assert
        for result in results {
            assert!(
                matches!(result, Err(CommitServiceError::BlocklistMismatch)),
                "{result:?}"
            );
        }
    }
}
