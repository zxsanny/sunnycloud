//! AC-1 and AC-4 (IT-13-1): the planner under seeded random failures against a modelled server.
//! Twenty seeds of 1,000 runs; each run draws block count, lane limit and failure probability. Runs with
//! 25,600 blocks are 1 in 100 so the whole file stays within the 60 s budget of a debug build.

mod common;

use std::time::Duration;

use common::{Noise, file_hash};
use sync_core_proto::{
    Action, BlockResult, CommitResult, RetryPolicy, UploadPlanner, effective_lanes,
    reuse_or_new_key,
};
use uuid::Uuid;

const SEEDS: u64 = 20;
const RUNS_PER_SEED: u64 = 1_000;
const LARGE_EVERY: u64 = 100;
const SMALL_BLOCKS: [u32; 2] = [1, 2];
const LARGE_BLOCKS: u32 = 25_600;
const LANES: [u32; 4] = [1, 2, 4, 8];
const FAILURES: [f64; 3] = [0.0, 0.1, 0.5];
const MAX_RESTARTS: u32 = 3;
const HASH: &str = "fe1f7bc3fa20f8b474a44830b7ab8404aa4762c3dbb363d5412164ceb47e6712";

const POLICY: RetryPolicy = RetryPolicy {
    base: Duration::from_millis(500),
    max: Duration::from_secs(30),
    max_attempts_per_block: 8,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Done,
    Restarted,
    GaveUp,
}

fn chance(noise: &mut Noise, p: f64) -> bool {
    ((noise.next_u64() >> 11) as f64) / ((1u64 << 53) as f64) < p
}

/// The server as the planner's executor sees it, plus the bookkeeping the invariants need.
struct Sim {
    planner: UploadPlanner,
    noise: Noise,
    fail: f64,
    cap: usize,
    /// Blocks the server holds.
    stored: Vec<bool>,
    /// Blocks the latest listing or `new` called needed and no `Acked` answered since.
    needed: Vec<bool>,
    sends: Vec<u32>,
    is_in_flight: Vec<bool>,
    in_flight: Vec<u32>,
    reread_pending: bool,
    commit_pending: bool,
}

impl Sim {
    fn new(noise: Noise, blocks: u32, lanes_max: u32, fail: f64, stored: Vec<bool>) -> Self {
        let n = blocks as usize;
        let mut sim = Self {
            planner: UploadPlanner::new(blocks, &[], lanes_max, POLICY),
            noise,
            fail,
            cap: effective_lanes(lanes_max, 4) as usize,
            stored,
            needed: vec![false; n],
            sends: vec![0; n],
            is_in_flight: vec![false; n],
            in_flight: Vec::new(),
            reread_pending: false,
            commit_pending: false,
        };
        sim.restart_planner(blocks, lanes_max);
        sim
    }

    fn missing(&self) -> Vec<u32> {
        (0..self.stored.len() as u32)
            .filter(|&i| !self.stored[i as usize])
            .collect()
    }

    fn restart_planner(&mut self, blocks: u32, lanes_max: u32) {
        let missing = self.missing();
        self.planner = UploadPlanner::new(blocks, &missing, lanes_max, POLICY);
        self.needed = self.stored.iter().map(|stored| !stored).collect();
        self.sends.fill(0);
        self.is_in_flight.fill(false);
        self.in_flight.clear();
        self.reread_pending = false;
        self.commit_pending = false;
    }

    fn needed_count(&self) -> usize {
        self.needed.iter().filter(|needed| **needed).count()
    }

    fn list_to_planner(&mut self) {
        let missing = self.missing();
        self.needed = self.stored.iter().map(|stored| !stored).collect();
        self.reread_pending = false;
        self.planner.on_session(&missing);
    }

    /// Run one planner life; returns how it ended.
    fn run(&mut self) -> Outcome {
        let n = self.stored.len();
        let resync = (4.0 / n as f64).min(0.02);
        let step_cap = 60 * n + 10_000;
        for _ in 0..step_cap {
            if let Some(outcome) = self.apply_actions() {
                return outcome;
            }
            if !self.commit_pending && chance(&mut self.noise, resync) {
                self.spontaneous_resync();
                continue;
            }
            self.deliver_one();
        }
        panic!("planner did not finish within {step_cap} steps");
    }

    fn apply_actions(&mut self) -> Option<Outcome> {
        let actions = self.planner.next_actions();
        let mut ended = None;
        for action in &actions {
            match *action {
                Action::SendBlock { i } => self.on_send(i),
                Action::Wait { ms } => assert!(ms > 0, "a zero wait must be omitted"),
                Action::RereadSession => {
                    assert!(!self.commit_pending);
                    self.reread_pending = true;
                }
                Action::Commit => self.on_commit_emitted(),
                Action::Done => {
                    assert!(self.stored.iter().all(|stored| *stored), "done with a hole");
                    ended = Some(Outcome::Done);
                }
                Action::RestartSession => ended = Some(Outcome::Restarted),
                Action::GiveUp { i } => {
                    assert!(self.sends[i as usize] >= POLICY.max_attempts_per_block);
                    ended = Some(Outcome::GaveUp);
                }
            }
        }
        assert!(
            self.in_flight.len() <= self.cap,
            "{} blocks in flight, cap {}",
            self.in_flight.len(),
            self.cap
        );
        if ended.is_some() {
            assert_eq!(self.planner.next_actions(), actions, "terminal is sticky");
            return ended;
        }
        let waiting = self.reread_pending || self.commit_pending || !self.in_flight.is_empty();
        assert!(
            waiting || !actions.is_empty(),
            "deadlock: no action, nothing to wait for"
        );
        None
    }

    fn on_send(&mut self, i: u32) {
        let index = i as usize;
        assert!(self.needed[index], "block {i} sent but not needed");
        assert!(!self.is_in_flight[index], "block {i} sent while in flight");
        assert!(!self.reread_pending, "sent while a listing is awaited");
        assert!(!self.commit_pending);
        self.sends[index] += 1;
        assert!(self.sends[index] <= POLICY.max_attempts_per_block);
        self.is_in_flight[index] = true;
        self.in_flight.push(i);
    }

    fn on_commit_emitted(&mut self) {
        assert_eq!(self.needed_count(), 0, "commit with a needed block");
        assert!(self.in_flight.is_empty(), "commit with a block in flight");
        assert!(!self.reread_pending);
        self.commit_pending = true;
    }

    fn spontaneous_resync(&mut self) {
        if chance(&mut self.noise, 0.3) {
            let held: Vec<usize> = (0..self.stored.len()).filter(|&i| self.stored[i]).collect();
            if !held.is_empty() {
                let lost = held[self.noise.below(held.len())];
                self.stored[lost] = false;
            }
        }
        self.list_to_planner();
    }

    fn deliver_one(&mut self) {
        let candidates = self.in_flight.len()
            + usize::from(self.reread_pending)
            + usize::from(self.commit_pending);
        let pick = self.noise.below(candidates);
        if pick < self.in_flight.len() {
            let i = self.in_flight.swap_remove(pick);
            self.is_in_flight[i as usize] = false;
            self.deliver_block_result(i);
        } else if self.reread_pending {
            self.list_to_planner();
        } else {
            self.deliver_commit_result();
        }
    }

    fn deliver_block_result(&mut self, i: u32) {
        let index = i as usize;
        let result = if !chance(&mut self.noise, self.fail) {
            self.stored[index] = true;
            self.needed[index] = false;
            BlockResult::Acked
        } else {
            let retry_after = chance(&mut self.noise, 0.5)
                .then(|| Duration::from_millis(1 + self.noise.below(5_000) as u64));
            match self.noise.below(50) {
                0 => BlockResult::SessionGone,
                1..=15 => BlockResult::HashRejected,
                16..=35 => {
                    self.stored[index] |= chance(&mut self.noise, 0.3);
                    BlockResult::Transient { retry_after }
                }
                _ => {
                    self.stored[index] |= chance(&mut self.noise, 0.5);
                    BlockResult::BlockConflict { retry_after }
                }
            }
        };
        self.planner.on_result(i, result);
    }

    fn deliver_commit_result(&mut self) {
        self.commit_pending = false;
        let missing = self.missing();
        let result = if !missing.is_empty() {
            self.needed = self.stored.iter().map(|stored| !stored).collect();
            CommitResult::NeedBlocks { list: missing }
        } else if chance(&mut self.noise, self.fail * 0.2) {
            if chance(&mut self.noise, 0.5) {
                CommitResult::SessionGone
            } else {
                CommitResult::BlocklistMismatch
            }
        } else {
            CommitResult::Ok
        };
        self.planner.on_commit(result);
    }
}

fn run_life(noise: &mut Noise, blocks: u32, lanes_max: u32, fail: f64) -> Outcome {
    let n = blocks as usize;
    let resumed = chance(noise, 0.3);
    let stored: Vec<bool> = (0..n).map(|_| resumed && chance(noise, 0.5)).collect();
    let mut sim = Sim::new(Noise(noise.next_u64() | 1), blocks, lanes_max, fail, stored);
    let key = Uuid::from_u128(u128::from(noise.next_u64()) | 1);
    let hash = file_hash(HASH);
    for restarts in 0..=MAX_RESTARTS {
        match sim.run() {
            Outcome::Restarted if restarts < MAX_RESTARTS => {
                let kept = reuse_or_new_key(Some((key, hash)), hash, || Uuid::from_u128(0));
                assert_eq!(kept, key, "a restart keeps the client key");
                sim.restart_planner(blocks, lanes_max);
            }
            outcome => return outcome,
        }
    }
    unreachable!("the loop returns on its last pass");
}

#[test]
fn invariants_hold_under_random_failures() {
    let mut tally = [0u32; 3];
    let mut no_failure_not_done = 0;
    for seed in 1..=SEEDS {
        let mut noise = Noise(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for run in 0..RUNS_PER_SEED {
            let blocks = if run % LARGE_EVERY == 0 {
                LARGE_BLOCKS
            } else {
                SMALL_BLOCKS[noise.below(SMALL_BLOCKS.len())]
            };
            let lanes_max = LANES[noise.below(LANES.len())];
            let fail = FAILURES[noise.below(FAILURES.len())];
            let outcome = run_life(&mut noise, blocks, lanes_max, fail);
            tally[outcome as usize] += 1;
            if fail == 0.0 && outcome != Outcome::Done {
                no_failure_not_done += 1;
            }
        }
    }
    assert_eq!(no_failure_not_done, 0, "without failures every run commits");
    assert!(tally.iter().all(|count| *count > 0), "outcomes {tally:?}");
}
