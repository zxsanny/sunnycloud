//! One upload session: the planner's actions run over the lanes, its results fed back, then the commit.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use sync_core_proto::{
    Action, BlockResult, CommitOutcome, CommitResult, Hash32, UploadPlanner, UploadSession,
    hash_block,
};
use tokio::task::JoinSet;
use uuid::Uuid;

use super::wire::{CommitBody, Sent, commit, get_upload, put_block, unexpected};
use super::{BLOCK, Restart, Run, block_len};
use crate::error::{Mapped, PlannerInput, SyncError};
use crate::hash_ops::identity_of;
use crate::host::{FileSource, UploadState};
use crate::request::RequestPath;
use crate::small::{blocking, joined};
use crate::tree_ops::into_error;

/// How a session ended without an error.
pub(super) enum SessionEnd {
    Committed(CommitOutcome),
    Restart(Restart),
    /// The commit answer was lost (no answer, or `upload_session_gone`): recovered by `start_upload` with the
    /// same request; the error is what the call returns when the recovery was already used.
    LostCommit(SyncError),
}

/// What a commit attempt gave.
enum CommitStep {
    Outcome(CommitOutcome),
    Planner(CommitResult),
    Lost(SyncError),
}

/// Bytes of blocks the server holds; only a 204 adds to it, and each block once per session.
struct Acked {
    counted: Vec<bool>,
    done: u64,
}

impl Acked {
    fn new(size: u64, block_count: u32, need_blocks: &[u32]) -> Self {
        let mut counted = vec![true; block_count as usize];
        for &index in need_blocks {
            if let Some(flag) = counted.get_mut(index as usize) {
                *flag = false;
            }
        }
        let done = (0..block_count)
            .filter(|&index| counted[index as usize])
            .map(|index| block_len(size, index))
            .sum();
        Self { counted, done }
    }

    /// Counts block `index`; `false` when it was counted already.
    fn add(&mut self, size: u64, index: u32) -> bool {
        match self.counted.get_mut(index as usize) {
            Some(flag) if !*flag => {
                *flag = true;
                self.done += block_len(size, index);
                true
            }
            _ => false,
        }
    }
}

impl Run<'_> {
    pub(super) async fn session(
        &mut self,
        session: &UploadSession,
        block_count: u32,
        lanes: u32,
    ) -> Result<SessionEnd, SyncError> {
        let size = self.opening.size;
        let mut planner =
            UploadPlanner::new(block_count, &session.need_blocks, lanes, self.up.policy);
        let mut acked = Acked::new(size, block_count, &session.need_blocks);
        let mut running = JoinSet::new();
        // The last failure of each block that is not acked yet: what a given-up block reports.
        let mut failures: HashMap<u32, SyncError> = HashMap::new();
        let mut restart = Restart::SessionGone;
        loop {
            let actions = planner.next_actions();
            if actions.is_empty() && running.is_empty() {
                return Err(SyncError::ProtocolViolation {
                    detail: "upload planner waits with nothing in flight",
                });
            }
            for action in actions {
                match action {
                    Action::SendBlock { i } => {
                        let hash = self.block_hash(i)?;
                        running.spawn(send_block(
                            Arc::clone(self.up.path),
                            Arc::clone(&self.request.source),
                            session.upload_id,
                            i,
                            hash,
                            size,
                        ));
                    }
                    Action::Wait { ms } => self.up.path.pause(self.wait(ms)).await?,
                    Action::RereadSession => {
                        match get_upload(self.up.path, session.upload_id).await {
                            Ok(need_blocks) => planner.on_session(&need_blocks),
                            Err(Mapped::Planner(PlannerInput::SessionGone)) => {
                                return Ok(SessionEnd::Restart(Restart::SessionGone));
                            }
                            Err(other) => return Err(into_error(other)),
                        }
                    }
                    Action::Commit => match self.commit(session.upload_id).await? {
                        CommitStep::Outcome(outcome) => return Ok(SessionEnd::Committed(outcome)),
                        CommitStep::Lost(error) => return Ok(SessionEnd::LostCommit(error)),
                        CommitStep::Planner(result) => {
                            if result == CommitResult::BlocklistMismatch {
                                restart = Restart::Rehash;
                            }
                            planner.on_commit(result);
                        }
                    },
                    Action::RestartSession => return Ok(SessionEnd::Restart(restart)),
                    Action::GiveUp { i } => {
                        return Err(failures
                            .remove(&i)
                            .unwrap_or_else(|| unexpected("block_given_up")));
                    }
                    // `Done` follows only a successful commit, which returned above.
                    Action::Done => {
                        return Err(SyncError::ProtocolViolation {
                            detail: "upload planner done without a commit",
                        });
                    }
                }
            }
            // A session listing or a commit answer was fed in: the planner has new actions.
            let Some(finished) = running.join_next().await else {
                continue;
            };
            let (index, sent) = joined(finished)?;
            let Sent { result, failure } = sent?;
            match failure {
                Some(failure) => {
                    failures.insert(index, failure);
                }
                None => {
                    failures.remove(&index);
                }
            }
            if result == BlockResult::Acked && acked.add(size, index) {
                self.progress.on_progress(acked.done, size);
            }
            planner.on_result(index, result);
        }
    }

    fn block_hash(&self, index: u32) -> Result<Hash32, SyncError> {
        self.request
            .hashed
            .block_hashes
            .get(index as usize)
            .copied()
            .ok_or(SyncError::ProtocolViolation {
                detail: "planner sent a block the file does not have",
            })
    }

    /// The planner's `Wait`, with jitter on top.
    fn wait(&self, ms: u64) -> Duration {
        planned_wait(ms, self.up.policy.base, self.up.jitter)
    }

    /// Checks the source, marks the checkpoint `Committing` and sends the commit once.
    async fn commit(&self, upload_id: Uuid) -> Result<CommitStep, SyncError> {
        if identity_of(&self.request.source).await? != self.opening {
            return Err(SyncError::SourceChanged);
        }
        self.save(Some(upload_id), UploadState::Committing).await?;
        let body = CommitBody {
            upload_id,
            parent: self.request.parent,
            name: &self.request.name,
            blocklist: &self.request.hashed.block_hashes,
            base_revision: self.request.base_revision,
            client_meta: self.request.client_meta.as_ref(),
            import_hint: self.request.import_hint,
        };
        Ok(match commit(self.up.path, &body).await {
            Ok(outcome) => CommitStep::Outcome(outcome),
            Err(Mapped::Planner(PlannerInput::NeedBlocks { list })) => {
                CommitStep::Planner(CommitResult::NeedBlocks { list })
            }
            Err(Mapped::Error(SyncError::BlocklistMismatch)) => {
                CommitStep::Planner(CommitResult::BlocklistMismatch)
            }
            Err(Mapped::Planner(PlannerInput::SessionGone)) => {
                CommitStep::Lost(unexpected("upload_session_gone"))
            }
            Err(Mapped::Error(error @ SyncError::Transport(_))) => CommitStep::Lost(error),
            Err(other) => return Err(into_error(other)),
        })
    }
}

/// `ms` (the server's `Retry-After` or the backoff) plus jitter drawn below `sync.retry_base`.
fn planned_wait(ms: u64, base: Duration, jitter: fn(Duration) -> Duration) -> Duration {
    Duration::from_millis(ms).saturating_add(jitter(base))
}

/// Reads block `index`, checks it against the hash the commit will carry, and sends it. A block that no longer
/// hashes to `expected` means the file changed since it was hashed: `SourceChanged`, nothing sent.
async fn send_block(
    path: Arc<RequestPath>,
    source: Arc<dyn FileSource>,
    upload_id: Uuid,
    index: u32,
    expected: Hash32,
    size: u64,
) -> (u32, Result<Sent, SyncError>) {
    let sent = async {
        let want = u32::try_from(block_len(size, index)).map_err(|_| SyncError::SourceChanged)?;
        let offset = u64::from(index) * BLOCK;
        let bytes =
            blocking(move || read_checked(source.as_ref(), offset, want, &expected)).await??;
        put_block(&path, upload_id, index, bytes, &expected).await
    };
    (index, sent.await)
}

fn read_checked(
    source: &dyn FileSource,
    offset: u64,
    want: u32,
    expected: &Hash32,
) -> Result<Bytes, SyncError> {
    let bytes = source
        .read_at(offset, want)
        .map_err(|error| SyncError::Io { kind: error.kind() })?;
    if bytes.len() as u64 != u64::from(want) {
        return Err(SyncError::SourceChanged);
    }
    match hash_block(&bytes) {
        Ok(hash) if &hash == expected => Ok(bytes),
        Ok(_) | Err(_) => Err(SyncError::SourceChanged),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Acked, BLOCK, planned_wait};

    #[test]
    fn a_wait_is_the_planned_delay_plus_the_injected_jitter() {
        // Arrange
        let jitter: fn(Duration) -> Duration = |base| base / 2;

        // Act
        let waited = planned_wait(3_000, Duration::from_millis(500), jitter);

        // Assert
        assert_eq!(waited, Duration::from_millis(3_250));
    }

    #[test]
    fn progress_starts_at_the_blocks_the_server_holds_and_counts_each_ack_once() {
        // Arrange
        let size = 3 * BLOCK + 10;
        let mut acked = Acked::new(size, 4, &[1, 3]);
        let start = acked.done;

        // Act
        let first = acked.add(size, 3);
        let again = acked.add(size, 3);
        let held = acked.add(size, 0);

        // Assert
        assert_eq!(start, 2 * BLOCK);
        assert!(first);
        assert!(!again);
        assert!(!held);
        assert_eq!(acked.done, 2 * BLOCK + 10);
    }
}
