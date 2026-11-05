//! The upload planner: a pure state machine that turns server results into the next actions.
//!
//! Sans-IO and clock-free: the executor (native `sync-core`, browser Worker) performs the actions, owns
//! timers and jitter, and feeds every result back. One planner serves one upload session; a terminal
//! action ([`Action::Done`], [`Action::RestartSession`], [`Action::GiveUp`]) is returned by every later
//! [`UploadPlanner::next_actions`] call and ends the planner, so a restart builds a new one with the same
//! `client_key` (the planner holds no key).
//!
//! In flight means: emitted as `SendBlock` and not yet answered by [`UploadPlanner::on_result`]. Results
//! for blocks the planner did not send, and for blocks the server no longer needs, are accepted as the
//! rules in [`UploadPlanner::on_result`] describe.

use std::collections::VecDeque;

use crate::{MAX_CLIENT_LANES, effective_lanes};

mod types;

use types::millis;
pub use types::{Action, BlockResult, CommitResult, RetryPolicy};

/// Where one block stands. The discriminant indexes `UploadPlanner::counts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Slot {
    /// Acked, or not needed by the server.
    Satisfied,
    /// Needed and waiting in the send queue.
    Queued,
    /// Sent, no answer yet, still needed.
    InFlight,
    /// Sent, no answer yet, and a session listing says the server no longer needs it.
    InFlightSatisfied,
    /// Answered `BlockConflict`; resolved by the next session listing.
    Conflict,
}

const SLOT_KINDS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reread {
    Idle,
    /// Emit `Wait{wait_ms}` and `RereadSession` at the next `next_actions`.
    Emit {
        wait_ms: u64,
    },
    /// `RereadSession` was emitted; `on_session` has not answered yet.
    Awaiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Sending,
    CommitPending,
    Ended(Action),
}

#[derive(Clone, Debug)]
pub struct UploadPlanner {
    policy: RetryPolicy,
    lanes: u32,
    slots: Vec<Slot>,
    /// `SendBlock`s emitted per block over the life of this planner.
    attempts: Vec<u32>,
    counts: [u32; SLOT_KINDS],
    /// Exactly the `Queued` blocks, each once, with the delay to wait before sending it.
    queue: VecDeque<(u32, u64)>,
    reread: Reread,
    phase: Phase,
}

impl UploadPlanner {
    /// A planner for a file of `block_count` blocks of which the server `need_blocks` lists the missing
    /// ones. `lanes_max` is the server's lane limit; at most `min(4, lanes_max)` blocks (at least one)
    /// are in flight. `policy.max_attempts_per_block` of zero counts as one.
    ///
    /// A listed index at or beyond `block_count` means the server's view and the file disagree: the
    /// planner is born ended with [`Action::RestartSession`].
    pub fn new(
        block_count: u32,
        need_blocks: &[u32],
        lanes_max: u32,
        mut policy: RetryPolicy,
    ) -> Self {
        policy.max_attempts_per_block = policy.max_attempts_per_block.max(1);
        let mut counts = [0; SLOT_KINDS];
        counts[Slot::Satisfied as usize] = block_count;
        let mut planner = Self {
            policy,
            lanes: effective_lanes(lanes_max, MAX_CLIENT_LANES).max(1),
            slots: vec![Slot::Satisfied; block_count as usize],
            attempts: vec![0; block_count as usize],
            counts,
            queue: VecDeque::new(),
            reread: Reread::Idle,
            phase: Phase::Sending,
        };
        planner.replace_needed(need_blocks);
        planner
    }

    /// The actions to perform now, in order; empty when the planner waits for results, a session
    /// listing or a commit answer.
    ///
    /// Order of precedence: a terminal action; a pending `Wait` + `RereadSession` (nothing is sent until
    /// the listing arrives); `SendBlock`s for free lanes, each preceded by `Wait` when its retry has a
    /// delay; `Commit` once no needed block is queued, in flight or in conflict. A block whose
    /// `max_attempts_per_block` sends are used up is never sent again: the list ends with
    /// [`Action::GiveUp`].
    pub fn next_actions(&mut self) -> Vec<Action> {
        match self.phase {
            Phase::Ended(action) => return vec![action],
            Phase::CommitPending => return Vec::new(),
            Phase::Sending => {}
        }
        match self.reread {
            Reread::Emit { wait_ms } => {
                self.reread = Reread::Awaiting;
                let mut actions = Vec::with_capacity(2);
                if wait_ms > 0 {
                    actions.push(Action::Wait { ms: wait_ms });
                }
                actions.push(Action::RereadSession);
                return actions;
            }
            Reread::Awaiting => return Vec::new(),
            Reread::Idle => {}
        }
        let mut actions = Vec::new();
        while self.in_flight() < self.lanes {
            let Some(&(i, delay_ms)) = self.queue.front() else {
                break;
            };
            if self.attempts[i as usize] >= self.policy.max_attempts_per_block {
                let give_up = Action::GiveUp { i };
                self.phase = Phase::Ended(give_up);
                actions.push(give_up);
                return actions;
            }
            self.queue.pop_front();
            if delay_ms > 0 {
                actions.push(Action::Wait { ms: delay_ms });
            }
            self.attempts[i as usize] += 1;
            self.set(i, Slot::InFlight);
            actions.push(Action::SendBlock { i });
        }
        if actions.is_empty() && self.is_complete() {
            self.phase = Phase::CommitPending;
            actions.push(Action::Commit);
        }
        actions
    }

    /// Feed the outcome of the `PUT` of block `i`.
    ///
    /// - `Acked`: the block is done.
    /// - `HashRejected`: the block is sent again, first in line, without delay.
    /// - `Transient{retry_after}`: sent again after `retry_after` as given, else the backoff of
    ///   [`RetryPolicy::backoff`] for the sends so far.
    /// - `BlockConflict{retry_after}`: the planner answers `Wait{retry_after or backoff}` then
    ///   `RereadSession`; [`Self::on_session`] then decides per block: still needed, resend (a resend
    ///   like `Transient`, bounded by the attempts); no longer needed and sent by this planner, acked;
    ///   otherwise `RestartSession`.
    /// - `SessionGone`: `RestartSession`.
    ///
    /// Every failed send counts against `max_attempts_per_block`, including `HashRejected`, so a
    /// persistently corrupting read ends in `GiveUp` instead of looping.
    ///
    /// Results are ignored when the planner has ended or `Commit` was emitted, and for blocks the
    /// server does not need (`Satisfied`), which is what a late answer to an already satisfied block
    /// looks like. A result for a block still in flight but no longer needed acks it whatever the
    /// result, as the server stores it. An index at or beyond the block count ends the planner with
    /// `RestartSession`.
    pub fn on_result(&mut self, i: u32, result: BlockResult) {
        if self.phase != Phase::Sending {
            return;
        }
        let Some(&slot) = self.slots.get(i as usize) else {
            self.phase = Phase::Ended(Action::RestartSession);
            return;
        };
        if result == BlockResult::SessionGone {
            self.phase = Phase::Ended(Action::RestartSession);
            return;
        }
        match slot {
            Slot::Satisfied => {}
            Slot::InFlightSatisfied => self.set(i, Slot::Satisfied),
            Slot::Queued | Slot::InFlight | Slot::Conflict => {
                self.on_needed_result(i, slot, result)
            }
        }
    }

    fn on_needed_result(&mut self, i: u32, slot: Slot, result: BlockResult) {
        match result {
            BlockResult::Acked => {
                self.unqueue(i, slot);
                self.set(i, Slot::Satisfied);
            }
            BlockResult::HashRejected => self.requeue(i, slot, 0),
            BlockResult::Transient { retry_after } => {
                let delay = self.delay_for(i, retry_after);
                self.requeue(i, slot, delay);
            }
            BlockResult::BlockConflict { retry_after } => {
                let wait_ms = self.delay_for(i, retry_after);
                self.unqueue(i, slot);
                self.set(i, Slot::Conflict);
                self.reread = match self.reread {
                    Reread::Idle => Reread::Emit { wait_ms },
                    Reread::Emit { wait_ms: pending } => Reread::Emit {
                        wait_ms: pending.max(wait_ms),
                    },
                    Reread::Awaiting => Reread::Awaiting,
                };
            }
            BlockResult::SessionGone => {}
        }
    }

    /// Feed a fresh session listing (`start_upload` / `get_upload` answer): the needed set becomes
    /// `need_blocks`.
    ///
    /// Queued blocks that are no longer listed are dropped and newly listed ones are queued (an acked
    /// block that is listed again is sent again). Blocks in flight stay in flight, so the lane count
    /// holds; one that is no longer listed is acked when its answer arrives. Blocks in `BlockConflict`
    /// are resolved as described at [`Self::on_result`]. An index at or beyond the block count ends the
    /// planner with `RestartSession`. Ignored once the planner ended or `Commit` was emitted.
    pub fn on_session(&mut self, need_blocks: &[u32]) {
        if self.phase == Phase::Sending {
            self.replace_needed(need_blocks);
        }
    }

    /// Feed the commit answer; ignored unless `Commit` is the last action emitted.
    ///
    /// `Ok` ends with `Done`. `NeedBlocks{list}` makes `list` the needed set; the planner sends those
    /// blocks, then emits `Commit` again. An empty list is a server inconsistency and ends with
    /// `RestartSession`. `BlocklistMismatch` and `SessionGone` end with `RestartSession`; the caller
    /// re-hashes first after `BlocklistMismatch`.
    pub fn on_commit(&mut self, result: CommitResult) {
        if self.phase != Phase::CommitPending {
            return;
        }
        match result {
            CommitResult::Ok => self.phase = Phase::Ended(Action::Done),
            CommitResult::BlocklistMismatch | CommitResult::SessionGone => {
                self.phase = Phase::Ended(Action::RestartSession);
            }
            CommitResult::NeedBlocks { list } if list.is_empty() => {
                self.phase = Phase::Ended(Action::RestartSession);
            }
            CommitResult::NeedBlocks { list } => {
                self.phase = Phase::Sending;
                self.replace_needed(&list);
            }
        }
    }

    /// Approximate heap and inline bytes held by the planner (PT-13-3).
    pub fn approx_state_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.slots.capacity() * std::mem::size_of::<Slot>()
            + self.attempts.capacity() * std::mem::size_of::<u32>()
            + self.queue.capacity() * std::mem::size_of::<(u32, u64)>()
    }

    fn in_flight(&self) -> u32 {
        self.counts[Slot::InFlight as usize] + self.counts[Slot::InFlightSatisfied as usize]
    }

    fn is_complete(&self) -> bool {
        self.counts[Slot::Queued as usize] + self.counts[Slot::Conflict as usize] + self.in_flight()
            == 0
    }

    fn set(&mut self, i: u32, slot: Slot) {
        let old = std::mem::replace(&mut self.slots[i as usize], slot);
        self.counts[old as usize] -= 1;
        self.counts[slot as usize] += 1;
    }

    fn unqueue(&mut self, i: u32, slot: Slot) {
        if slot == Slot::Queued {
            self.queue.retain(|&(queued, _)| queued != i);
        }
    }

    /// Send `i` again, first in line, after `delay_ms`.
    fn requeue(&mut self, i: u32, slot: Slot, delay_ms: u64) {
        self.unqueue(i, slot);
        self.set(i, Slot::Queued);
        self.queue.push_front((i, delay_ms));
    }

    fn delay_for(&self, i: u32, retry_after: Option<std::time::Duration>) -> u64 {
        millis(retry_after.unwrap_or_else(|| self.policy.backoff(self.attempts[i as usize])))
    }

    /// Make `need_blocks` the needed set. See [`Self::on_session`].
    fn replace_needed(&mut self, need_blocks: &[u32]) {
        let mut wanted = vec![false; self.slots.len()];
        for &i in need_blocks {
            match wanted.get_mut(i as usize) {
                Some(flag) => *flag = true,
                None => {
                    self.phase = Phase::Ended(Action::RestartSession);
                    return;
                }
            }
        }
        let mut newly_queued = Vec::new();
        for (index, &want) in wanted.iter().enumerate() {
            let i = index as u32;
            match (self.slots[index], want) {
                (Slot::Satisfied | Slot::Conflict, true) => {
                    self.set(i, Slot::Queued);
                    newly_queued.push(i);
                }
                (Slot::Queued, false) => self.set(i, Slot::Satisfied),
                (Slot::InFlight, false) => self.set(i, Slot::InFlightSatisfied),
                (Slot::InFlightSatisfied, true) => self.set(i, Slot::InFlight),
                (Slot::Conflict, false) if self.attempts[index] > 0 => {
                    self.set(i, Slot::Satisfied);
                }
                (Slot::Conflict, false) => {
                    self.phase = Phase::Ended(Action::RestartSession);
                    return;
                }
                _ => {}
            }
        }
        let slots = &self.slots;
        self.queue
            .retain(|&(i, _)| slots[i as usize] == Slot::Queued);
        self.queue.extend(newly_queued.into_iter().map(|i| (i, 0)));
        self.reread = Reread::Idle;
    }
}
