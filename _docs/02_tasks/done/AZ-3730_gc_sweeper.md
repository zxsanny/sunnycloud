# GcSweeper with claim lease, owner cleanup and drain budget

**Task**: AZ-3730_gc_sweeper
**Name**: GC sweeper
**Description**: The janitor that removes due blobs: claims them with a lease, unlinks files outside any transaction, has every row owner delete its rows, and drains a backlog within one cycle budget
**Complexity**: 5 points
**Dependencies**: AZ-3729_blob_registry_gc_queue, AZ-3768_contracts_part1
**Component**: 04 blobstore
**Tracker**: AZ-3730
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c10

## Problem

Blobs queued by deletes, replacements, account erase and derived removal must go only after their `due_at` (AC-30) and, for account deletion, at once (AC-27). Rows in 02 (`revisions`) and 08 (`derived`) point at a blob, but 04 may not depend on those crates (ADR-015). A crash in the middle of a removal must not leave a row whose file is gone or a file nobody owns. A large backlog must not take one batch per `gc_interval` to drain.

## Outcome

- `GcSweeper::run_cycle(now)` (a `Janitor`) repeats batches of `gc_batch` until no row is due or `gc_cycle_budget` (default 30 s) is spent, and returns `GcStats{removed, remaining_due, budget_spent}`.
- A batch: a short transaction claims up to `gc_batch` due rows by moving `due_at` forward by `gc_claim_lease`; outside any transaction each file is unlinked (an already missing file is logged at WARN, not an error); a second short transaction calls `remove_owner_rows(tx, blob_id)` of every registered `BlobOwnerCleanup`, then `remove_row_in_tx`.
- INFO log at cycle start and with the cycle summary.

## Scope

### Included
- The sweeper cycle, claim lease, unlink, cleanup call order, budget, stats, logs.

### Excluded
- The `BlobOwnerCleanup` implementations (E-13/c13, E-18/c10); tests here use a recording implementation. Scheduling the janitor (E-22/c06) and `admin gc-run` (E-22/c10).

## Acceptance Criteria

**AC-1: Nothing removed early**
Given 270 queued blobs and the clock at retention minus 1 ms
When a cycle runs
Then 0 blobs are removed

**AC-2: A backlog drains in one cycle**
Given `gc_batch = 10`, `gc_cycle_budget = 5 s`, 250 blobs due and 20 due in the future
When one cycle runs after the due time
Then all 250 are removed in 25 batches, `GcStats` is `{removed: 250, remaining_due: 0, budget_spent: false}`, and the 20 future blobs stay

**AC-3: Budget ends the cycle**
Given more due blobs than one budget can remove
When a cycle runs
Then it stops when the budget is spent, returns `budget_spent: true` with `remaining_due` above 0, and the rest is removed by the next cycle

**AC-4: Missing file is not an error**
Given a due blob whose file is already gone
When it is swept
Then a WARN line is logged, its owner cleanup runs and its rows are removed

**AC-5: Owner cleanup before row removal**
Given a recording `BlobOwnerCleanup`
When 250 blobs are swept
Then cleanup is called exactly once per blob, inside the second transaction, before that blob's `remove_row_in_tx`

**AC-6: A crash is healed by the lease**
Given a decorator that fails one batch between unlink and the cleanup transaction
When the clock passes `gc_claim_lease` and a second cycle runs
Then the claimed rows are claimable again and the second cycle completes them; afterwards no blob row exists whose file is gone and no file exists whose row was removed

**AC-7: No transaction across the unlink**
Given a hook in the unlink step
When the sweeper is inside it
Then PostgreSQL shows 0 sessions idle in a transaction for the sweeper

## Non-Functional Requirements

**Performance**
- A backlog drains within one cycle of `gc_cycle_budget` (30 s default) instead of one batch per `gc_interval`.

**Reliability**
- Claimed rows survive a process death (lease, no leader); janitors share no in-memory state.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Batch loop termination when nothing is due | Loop ends; stats correct |
| AC-3 | Budget check between batches | Stops with `budget_spent: true` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 270 queued, clock before due | IT-04-10 early case | 0 removed | FT-P-16 |
| AC-2 | 250 due, 20 future, batch 10 | IT-04-10 drain | 250 removed in one cycle; 20 stay | NFT-RES-09 |
| AC-3 | Backlog larger than budget | Budget case (unit list: "GC cycle drains a backlog larger than `gc_batch`") | `budget_spent: true`, rest next cycle | |
| AC-4 | One file missing | IT-04-10 missing-file case | WARN; rows removed | |
| AC-5 | Recording cleanup | IT-04-10 cleanup order | Once per blob, before row removal | FT-P-19, NFT-SEC-27 |
| AC-6 | Decorator fails one batch | IT-04-10 interrupted batch | Healed after lease; no orphan row or file | |
| AC-7 | Unlink hook | IT-04-10 transaction check | 0 idle-in-transaction | |

## Constraints

- System scope for the sweeper's queries (ADR-014); no transaction across unlink or any file I/O.
- The cleanup list is registered late by 12; 04 has no crate edge to 02 or 08.
- `GcStats` is reported through `CycleStats` of `contracts` once its fields are fixed (open in contracts part 1).

## Risks & Mitigation

**Risk 1: Backup ordering versus GC (R33)**
- *Risk*: a backup copies the database after GC removed files it still references.
- *Mitigation*: retention minus GC interval must exceed the copy time; documented in E-22's backup procedure and checked by the doctor retention check (E-10/c13).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `CycleStats` fields in `contracts` are open (contracts part 1, open point 1); `GcStats` maps onto them once fixed | E-00 | open | Low |

## Contract

This task produces/implements the sweeper part of the contract at `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md` (including the obligations of `BlobOwnerCleanup` implementers).
Consumers MUST read that file — not this task spec — to discover the interface.
