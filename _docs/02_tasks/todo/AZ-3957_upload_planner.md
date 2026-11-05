# UploadPlanner state machine and property tests

**Task**: AZ-3957_upload_planner
**Name**: Upload planner
**Description**: The pure state machine that decides which block to send next, how to react to each server result, when to commit and when to restart, shared by the native executor and the browser Worker
**Complexity**: 5 points
**Dependencies**: AZ-3955_sync_core_proto
**Component**: 13_sync_core (crate `sync-core-proto`)
**Tracker**: AZ-3957
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `UploadPlanner`, `Action`, `BlockResult`, `CommitResult`, `RetryPolicy` (this task fills the planner part).
- `_docs/02_document/contracts/03_transfer/upload_service.md` — `put_block` outcomes and lane limit.

## Problem

Resume, lanes and retries decide AC-3 (re-send at most 10 MiB), AC-7 (a corrupt block is re-sent), AC-38/AC-68 (ack only on 204) and AC-52 (bounded memory). The rules are subtle: a 409 `BlockConflict` can mean "still in flight", "already stored with my bytes" or "stored with other bytes". If each executor coded them, the browser and native clients would drift. 13 §2 defines the planner as a pure state machine; nothing implements it yet.

## Outcome

- `UploadPlanner` with `new`, `next_actions`, `on_result`, `on_session`, `on_commit` as in the contract.
- Property tests over 20 seeds x 1,000 runs and the reaction table of IT-13-2 pass.
- The terminal behaviour after `max_attempts_per_block` is decided and recorded (contract open point 2).

## Scope

### Included
- Lane accounting: at most `min(4, lanes_max)` blocks in flight.
- Resume diff: only blocks listed in `need_blocks` are sent; `on_session` replaces the needed set.
- Reactions: `Acked`; `HashRejected` (resend); `Transient{retry_after?}` (backoff from `RetryPolicy`, bounded by `max_attempts_per_block`); `BlockConflict` (wait, re-read, then resend / treat as acked / restart); `SessionGone` (restart, same key).
- Commit: emitted only when no needed block is outstanding; `on_commit(NeedBlocks)` sends the listed blocks, then commits again; `BlocklistMismatch` and `SessionGone` restart.
- The give-up action when a block exhausts its attempts, recorded in the contract.

### Excluded
- Jitter, real waiting and HTTP (E-23/c09b executor; the planner returns `Wait{ms}` without a clock).
- Re-hashing after `BlocklistMismatch` (E-23/c09a, c09b).
- The WASM binding (E-23/c16).

## Acceptance Criteria

**AC-1: Lane and loss invariants under random failures (AC-3, AC-52)**
Given a seeded driver, 20 seeds x 1,000 runs, `block_count` in {1, 2, 25,600}, `lanes_max` in {1, 2, 4, 8} and results drawn from `Acked | HashRejected | Transient | BlockConflict | SessionGone` with failure probability 0, 0.1 and 0.5
When the driver runs each plan to its end
Then at every step at most `min(4, lanes_max)` blocks are in flight, no acked block is re-sent unless `on_session` re-lists it, and every needed block ends acked or the run ends in `RestartSession` or `Done`

**AC-2: Retry bound**
Given `max_attempts_per_block = 8`
When one block gets 8 `Transient` results
Then the planner stops retrying that block and emits the recorded terminal action instead of a ninth `SendBlock` for it

**AC-3: Reactions to each wire result (AC-7)**
Given the cases of IT-13-2: (a) `on_session([5, 6, 9])` of 100 blocks; (b) `HashRejected` on block i; (c) `BlockConflict{retry_after = 2 s}` with i still needed; (d) `BlockConflict`, i no longer needed and sent by this run; (e) the same, not sent by this run; (f) `on_commit(NeedBlocks{[7]})` and `on_commit(BlocklistMismatch)`; (g) `SessionGone`
When each case is fed to a planner
Then (a) only 5, 6, 9 are sent; (b) i is sent again; (c) `Wait{2000}`, then `RereadSession`, then i is resent; (d) i counts as acked; (e) `RestartSession`; (f) `SendBlock{7}` then `Commit`, and `RestartSession`; (g) `RestartSession`

**AC-4: No early commit (AC-38, AC-68 client share)**
Given any sequence of results
When the planner emits `Commit`
Then no needed block is outstanding or unacked at that moment

**AC-5: Cost at 50 GiB (AC-4)**
Given `new(25,600, all, 4, policy)` and 25,600 `Acked` results
When `next_actions` is called for the whole plan
Then the mean cost per call is at most 50 us and the planner state is at most 4 MB (failure above 1 ms or 16 MB)

## Non-Functional Requirements

**Performance**
- PT-13-3 targets: `next_actions` mean <= 50 us, state <= 4 MB at 25,600 blocks (targets proposed by the 13 tests).

**Reliability**
- Pure and deterministic: the same inputs give the same actions on native and wasm32.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Property test, 20 seeds x 1,000 runs | Invariants hold (IT-13-1) |
| AC-2 | 8 `Transient` on one block | No ninth send; terminal action |
| AC-3 | Cases (a)..(g) | Documented action per case (IT-13-2) |
| AC-4 | `Commit` emission check inside the property test | Never with a needed block outstanding |
| AC-5 | Micro-benchmark at 25,600 blocks | <= 50 us mean, <= 4 MB (PT-13-3) |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Planner only, no network | IT-13-1 property run | Pass within 60 s | AC-3, AC-52 |
| AC-3 | Planner only | IT-13-2 table | Pass within 2 s | FT-N-01, FT-P-05 |

## Constraints

- Sans-IO: no clock, no randomness; `Wait{ms}` carries `retry_after` as given, or the backoff computed from `RetryPolicy` and the attempt count.
- The planner takes `lanes_max` as an input; it does not decide the lane count (G-03-1, E-23/c03).

## Risks & Mitigation

**Risk 1: A `BlockConflict` reading that loses data**
- *Risk*: treating a block as acked when the server stored other bytes.
- *Mitigation*: only "not needed and sent by this run" counts as acked; every other case restarts, and the commit-time blocklist check is the final guard (13 §2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The planner action set has no "give up" action for a block that exhausted `max_attempts_per_block`; this task decides it and records it in the contract (open point 2) | E-23 | open | Low |
| G-03-1 (16 MiB worst case vs AC-3's 10 MiB) may change `lanes_max` or add an in-flight cap input; the planner stays parameterised and does not decide it | Owner (via E-14/c01, E-23/c03) | open | Medium |

## Contract

This task implements the `UploadPlanner` part of the contract at `_docs/02_document/contracts/13_sync_core/sync_core_proto.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
