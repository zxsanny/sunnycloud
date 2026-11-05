use std::time::Duration;

/// What the executor does next. Returned by [`UploadPlanner::next_actions`](super::UploadPlanner::next_actions).
///
/// The executor runs a returned list in order. `Wait` pauses the rest of that list (sends in flight
/// continue), then the executor calls `next_actions` again after each result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Upload block `i` (`PUT` of the block, ack only on 204).
    SendBlock { i: u32 },
    /// Sleep `ms` before the actions that follow in the same list. Carries the server's `retry_after`
    /// as given, or the backoff computed from [`RetryPolicy`]; jitter is the executor's.
    Wait { ms: u64 },
    /// Call `get_upload` and feed the answer to `on_session`.
    RereadSession,
    /// All needed blocks are acked and nothing is in flight: commit, then feed `on_commit`.
    Commit,
    /// Terminal: the commit succeeded.
    Done,
    /// Terminal: abandon this planner, start a new session with the same `client_key` and build a new
    /// planner from the answer. After `BlocklistMismatch` the caller re-hashes the file first.
    RestartSession,
    /// Terminal: block `i` used up `max_attempts_per_block` sends without an ack, so the planner stops
    /// retrying it instead of emitting another `SendBlock`.
    ///
    /// Decision for `sync_core_proto.md` open point 2: the executor stops the transfer, cancels the
    /// requests in flight, keeps the checkpoint and the `client_key`, and reports a retryable
    /// "block upload failed" error to its caller. A later run starts with a new planner (attempt counts
    /// start again), so the file is retried only when the host schedules it.
    GiveUp { i: u32 },
}

/// Outcome of one block `PUT`, as mapped by the executor from the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockResult {
    /// 204: the block is durable on the server.
    Acked,
    /// 422 `block_hash_mismatch`: the bytes arrived changed; the block is sent again.
    HashRejected,
    /// A retryable failure (timeout, reset, 429, 5xx); `retry_after` is the server's hint if any.
    Transient { retry_after: Option<Duration> },
    /// 409 `block_conflict`: another request for the block is in flight, or the server stores the block
    /// from another request. The planner answers `Wait`, `RereadSession`, then decides from the session.
    BlockConflict { retry_after: Option<Duration> },
    /// 410 `upload_session_gone`.
    SessionGone,
}

/// Outcome of the commit call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitResult {
    /// 409 `need_blocks`: the server lists the blocks it still lacks.
    NeedBlocks { list: Vec<u32> },
    /// 422 `BlocklistMismatch`: the file changed or the blocklist disagrees.
    BlocklistMismatch,
    /// The commit succeeded.
    Ok,
    /// 410 `upload_session_gone`.
    SessionGone,
}

/// Retry limits, from `sync.retry_base`, `sync.retry_max` and `sync.max_attempts_per_block`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub base: Duration,
    pub max: Duration,
    /// Sends allowed per block over the life of one planner; the next one is [`Action::GiveUp`].
    /// Zero is treated as one.
    pub max_attempts_per_block: u32,
}

impl RetryPolicy {
    /// Delay before the next send of a block that has been sent `attempts` times:
    /// `min(max, base * 2^(attempts - 1))`.
    pub fn backoff(&self, attempts: u32) -> Duration {
        let shift = attempts.saturating_sub(1).min(31);
        self.base.saturating_mul(1u32 << shift).min(self.max)
    }
}

pub(super) fn millis(delay: Duration) -> u64 {
    u64::try_from(delay.as_millis()).unwrap_or(u64::MAX)
}
