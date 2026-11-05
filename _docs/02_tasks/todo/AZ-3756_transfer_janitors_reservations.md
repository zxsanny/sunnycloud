# Session reaper, receipt janitor, idle flush and ReservationSource

**Task**: AZ-3756_transfer_janitors_reservations
**Name**: Transfer janitors and reservation source
**Description**: Expire idle sessions and staged rows releasing exactly their reservation, delete old receipts, checkpoint and evict idle progress entries, and report the sum of open reservations for the drift check
**Complexity**: 3 points
**Dependencies**: AZ-3736_transfer_resume_abort, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3768_contracts_part1, AZ-3729_blob_registry_gc_queue, AZ-3750_quota_ledger, AZ-3752_quota_recompute, AZ-3716_tree_revision_lookups
**Component**: 03_transfer
**Tracker**: AZ-3756
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md` — this task adds `ReservationSource`.

## Problem

Abandoned sessions and staged files hold quota and disk until someone removes them; the removal must give back exactly the reserved bytes once, even when a commit races it (AC-68, AC-74). Receipts must not grow forever, idle progress must reach the database before it is evicted, and the quota drift check needs the sum of open reservations from the owner of the rows.

## Outcome

- `SessionReaper::run_cycle` deletes sessions and staged rows past `expires_at` and releases exactly the returned `reserved_bytes`; files are discarded after the commit of that transaction.
- `ReceiptJanitor::run_cycle` deletes receipts older than `commit_receipt_ttl`.
- `ProgressTable::flush_idle` checkpoints and evicts entries idle longer than the checkpoint interval.
- `ReservationSource::open_reserved_bytes(account)` equals 07a's reserved bytes.

## Scope

### Included
- Reaper per batch: one short transaction with `DELETE … RETURNING blob_id, reserved_bytes`, `QuotaLedger::release_reservation` for that amount, `BlobRegistry::remove_row_in_tx`; discard after COMMIT; rows claimed without a leader; `upload_sessions_total{result = expired}`.
- Rolling TTL from the last checkpoint.
- Receipt janitor and idle flush as `Janitor`s; scheduler registration is E-22/c06.
- `ReservationSource::open_reserved_bytes`.

### Excluded
- Abort (E-14/c06); the erase hook (E-14/c12).

## Acceptance Criteria

**AC-1: Expiry releases exactly once (AC-68, AC-74)**
Given `upload_session_ttl = 400 ms`, 20 open sessions of which 10 receive a block at +300 ms, and 20 staged rows
When the reaper runs after the TTL, and runs again
Then the 10 untouched sessions and the staged rows are deleted, reserved bytes fall by exactly their sum, files are discarded only after the delete committed, the touched sessions survive, and the second run releases nothing more

**AC-2: Reaper versus commit (AC-68, AC-74)**
Given one expiring session
When `SessionReaper::run_cycle` and `commit_upload` race on it 500 times
Then each iteration has exactly one winner: commit won gives used = size, reserved 0, blob committed; reaper won gives used 0, reserved 0, blob row and file removed; reserved never goes below 0 and used + reserved never exceeds the expected value

**AC-3: Receipts expire by age**
Given 100 receipts aged 0 to 2 x `commit_receipt_ttl`
When the receipt janitor runs
Then only receipts older than the TTL are deleted, and a replay of a deleted commit id is treated as a new batch

**AC-4: Idle flush**
Given sessions idle longer than `checkpoint_interval`
When `flush_idle` runs
Then their `received` and `block_hashes` are written to the rows and the entries are evicted

**AC-5: Reservation source has no drift (AC-74)**
Given random starts, aborts, commits and expiries on one account
When `open_reserved_bytes` and `QuotaAdmin::view.reserved_bytes` are read, and `QuotaAdmin::recompute(live_bytes, reserved_bytes)` runs with `LiveUsageSource` and this source
Then the two reserved values are equal and `recompute` changes nothing

**AC-6: Uncommitted upload after a crash (AC-68)**
Given all blocks acked and the service dropped before `commit_upload`, never committed afterwards
When the TTL passes and the reaper runs
Then reserved bytes return to 0, 0 revisions and 0 nodes exist for it

## Non-Functional Requirements

**Performance**
- A cycle with nothing due writes nothing.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Rolling expiry computation | `last_checkpoint_at + ttl` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 20 sessions, 20 staged, 400 ms TTL | Reaper twice (IT-03-10) | Exact release once; touched survive | FT-N-40 |
| AC-2 | Expiring session | 500 races (IT-03-9 case b) | One winner; no negative reserve | NFT-RES-34, NFT-RES-38 |
| AC-3 | 100 aged receipts | Janitor (IT-03-10) | Only old ones deleted | |
| AC-4 | Idle sessions | `flush_idle` (IT-03-10) | Checkpointed, evicted | AC-3 |
| AC-5 | Random history | Two sources, `recompute` (IT-03-12) | Equal; no-op | |
| AC-6 | Acked, never committed | TTL and reaper (IT-03-17 case a) | Reserved 0; no revision | NFT-RES-04, NFT-RES-34 |

## Constraints

- Real 02, 04, 07a on one PostgreSQL; `Janitor` trait from `contracts` (E-00/c03). The scheduler-driven run is in pipelines AZ-3655 and AZ-3656.

## Risks & Mitigation

**Risk 1: Reaper and commit both release (AC-74)**
- *Risk*: double release drives reserved bytes negative.
- *Mitigation*: whoever deletes the row releases; a commit that lost the row rolls back its own release (AC-2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
