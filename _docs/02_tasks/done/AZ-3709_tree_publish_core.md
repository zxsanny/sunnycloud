# TreeCommit publish core: guard, lock, effects, savepoints, stamp last

**Task**: AZ-3709_tree_publish_core
**Name**: Tree publish core
**Description**: Implement `TreeCommit::publish` for single and batch commits: account guard first, account lock, the `CommitEffect` protocol, one savepoint per entry, the basic outcomes, and the feed stamp as the last statement
**Complexity**: 5 points
**Dependencies**: AZ-3706_tree_skeleton, AZ-3708_tree_folder_mutations, AZ-3729_blob_registry_gc_queue, AZ-3750_quota_ledger, AZ-3775_feed_stamp, AZ-3776_change_feed_read, AZ-3777_feed_long_poll
**Component**: 02_tree
**Tracker**: AZ-3709
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.
- `_docs/02_document/contracts/02_tree/tree_commit.md` — produced by this task (E-13/c06, c07, c08 extend it).

## Problem

Every upload commit, single or a 500-entry batch, must land in one transaction that touches node, revision, quota and feed counter together, with the counter bump as the very last statement so concurrent commits leave no gap (AC-69). 03 and 08 must be able to add their own writes (blob flip, session row delete, receipt, job enqueue) inside that transaction without opening it themselves (ADR-015), and one bad entry must not sink the other 499.

## Outcome

- `publish` runs the fixed sequence: guard, advisory lock, `on_begin`, per-entry savepoints with `on_entry`, `on_batch`, stamp, COMMIT, notify.
- A failed entry rolls back only its savepoint, including its quota release and its effects; the transaction stays usable.
- A receipt effect can replay a stored response under the lock, and an all-`NoChange` call stamps nothing.

## Scope

### Included
- `publish(CommitActor, Vec<PublishEntry>, Vec<CommitEffect>, now)` returning `Published(Vec<EntryResult>)` or `Replayed(stored)`.
- Whole-call errors `AccountNotActive`, `CommitIdReused` (from an effect), `EffectFailed` (batch effect); per-entry errors `NameInvalid`, `ParentDeleted`, `ParentNotFound`, `NotAFolder`, `NotFound`, `EffectFailed(code)`.
- Targets: `Existing{node_id, base_revision?}` addressed by node id, `New{parent_id, name}`; outcomes `Created`, `Updated`, `Replaced`, `NoChange{existing_revision}` with their node and revision writes.
- Inside each entry savepoint: `QuotaLedger::adjust_used` with the entry's delta (for `Created`: +size) and `QuotaLedger::release_reservation(reserved_bytes)`, then every effect's `on_entry` with `PublishedEntry` (node, revision, content hash, media type, size, `import_hint`, `vault`).
- `on_batch` with every entry outcome, failed ones included; `FeedStamp::stamp` with the union of touched nodes (one seq per call) as the last statement; no stamp or notify when nothing was touched.

### Excluded
- `ConflictCopied`, `CreatedAsCopy`, `Revived` (E-13/c06); the full delta matrix and GC queueing (E-13/c07); the blob-ownership check (E-13/c08).
- The real 03, 08 and 01 counterparts (E-13/c16).

## Acceptance Criteria

**AC-1: Basic outcomes and the entry passed to effects**
Given a recording effect
When `publish` runs a `New` entry, an `Existing` entry with base = current, the same content hash again, and an entry with `vault = true`
Then the results are `Created` (node and revision rows, one stamp), `Updated` (same node id, new current revision, old revision row kept), `NoChange{existing_revision}` (no stamp, no quota change, `on_batch` still runs), and the effect receives node, revision, hash, media type, size, `import_hint` and `vault = true`

**AC-2: Statement order and stamp last (AC-69)**
Given the PostgreSQL statement log and the effect call log
When a normal batch is published
Then the order is guard < advisory lock < `on_begin` < per-entry savepoints < `on_batch` (with all outcomes) < stamp < COMMIT, there are 0 statements between stamp and COMMIT, and notify runs only after COMMIT

**AC-3: Lock held while effects run**
Given an effect whose `on_begin` pauses
When a second connection tries the account's advisory lock during the pause
Then the try returns false

**AC-4: Replay**
Given an effect whose `on_begin` returns `Replay(response)`
When `publish` is called
Then it returns `Replayed(response)` with no node write, no stamp and no quota call

**AC-5: Batch effect failure**
Given an effect whose `on_batch` returns a `Batch` error
When `publish` is called
Then the whole transaction rolls back and no seq is consumed

**AC-6: All entries NoChange**
Given a batch whose every entry is `NoChange` and an effect that writes a row in `on_batch`
When `publish` is called
Then there is no stamp and no notify, and the effect's row is committed

**AC-7: Savepoint per entry in a 500-entry batch (AC-8, AC-74)**
Given 500 `New` entries where entry 10 has a tombstoned parent, entry 250 an invalid name, and entry 400 gets `EffectError::Entry(UploadNotFound)` from the effect after its reservation release
When the batch is published
Then the result holds 497 `Published` and 3 typed failures (`ParentDeleted`, `NameInvalid`, `EffectFailed(code)`), the counter advanced by exactly 1, all 497 nodes carry the same seq, entry 400's reservation is still reserved while the others are released, and no "transaction is aborted" error occurs

**AC-8: No revision names an uncommitted blob (tree half of AC-68)**
Given 300 publishes whose test effect calls the real `BlobRegistry::mark_committed_in_tx` and fails with probability 0.3 (`Entry` or `Batch`), plus one publish naming a `blob_id` with no `blobs` row
When the invariant probe runs after every publish
Then 0 committed revisions reference a missing or non-committed blob, and the dangling id gives a typed entry failure with no revision row and no panic

**AC-9: Fifty concurrent commits, feed exactly once (AC-69)**
Given 50 tasks that each publish one new file at a barrier, then 50 tasks that each update a different file twice, and a reader paging the change feed from cursor 0 with limit 7 during and after the writes
When the run is repeated 100 times
Then round A yields seq 1..50 each exactly once, round B ends at seq 150 with every node's seq equal to its last commit, reader pages end on seq boundaries and cover each node once, and there are 0 failures

**AC-10: Guard first**
Given the `AccountGuard` port reports the account not active
When `publish` is called
Then it returns `AccountNotActive` for the whole call with 0 rows, no quota change and no seq consumed

## Non-Functional Requirements

**Performance**
- Hot account p95 at most 250 ms (fail 600 ms) with 50 writers and a 500-entry batch p95 at most 1.0 s (fail 2.0 s); measured by PT-02-3 and PT-02-4 in E-13/c15.

**Reliability**
- Effects do no I/O and add no statement after the stamp; a deadlock is retried once.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Touched-node union per call | One stamp with the union; none when empty |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Recording effect | New, Existing, same hash, vault flag (IT-02-5) | Created, Updated, NoChange; effect payload complete | AC-34 |
| AC-2 | Statement log on | Normal run (IT-02-10) | Fixed order; 0 statements after stamp | AC-69 |
| AC-3 | Pausing `on_begin` | Try-lock from second connection (IT-02-10) | Lock held | |
| AC-4 | Replay effect | Publish (IT-02-10) | `Replayed`, nothing applied | AC-40 |
| AC-5 | Batch-failing effect | Publish (IT-02-10) | Full rollback, no seq | |
| AC-6 | All `NoChange` | Publish (IT-02-10) | No stamp; effect row committed | |
| AC-7 | 500 entries, 3 bad | Publish (IT-02-9) | 497 + 3 typed; one seq; release of 400 rolled back | PT-02-4 |
| AC-8 | Random effect failures | Invariant probe (IT-02-12) | 0 dangling revisions | NFT-RES-34 |
| AC-9 | 50 writers, reader | Rounds A and B x100 (IT-02-13) | Gapless, once each | NFT-RES-35 |
| AC-10 | Guard refuses | Publish (IT-02-10) | `AccountNotActive`, nothing changed | |

## Constraints

- Test effects (recording, failing, replay) are per-call ports supplied by the tests; quota, feed and blob registry are the real E-11, E-12 and E-10 implementations; `AccountGuard` uses a port-level test implementation until E-13/c16.
- The account advisory lock is taken only here, by moves (E-13/c04) and by restore (E-13/c11).

## Risks & Mitigation

**Risk 1: Per-account serialisation (R07)**
- *Risk*: the lock plus a 500-entry batch holds node locks for the whole transaction.
- *Mitigation*: batch limit bounds the hold; gates NFT-PERF-09 and PT-02-3.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `Updated` versus `Replaced` are both listed outcomes but 02 §2 does not define the difference (IT-02-5 says "then Replaced when the old revision is non-current") | E-13 owner | open | Medium |
| `contracts` part 2 open point 3: 08's media effect needs the account id, which `PublishedEntry` does not carry; E-13 decides between adding `account_id` and reading it from `CommitActor` (a `contracts` change, Layout Rule 6) | E-13 owner | open | Medium |
| IT-02-12's dangling `blob_id` case expects a mapped FK failure, while E-13/c08 maps unknown blobs to `BlocklistMismatch`/`NotFound` before the insert; the two must give one error | E-13 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/02_tree/tree_commit.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
