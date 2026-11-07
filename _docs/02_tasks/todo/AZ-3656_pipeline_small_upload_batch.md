# Small-file upload and batch commit pipeline (F5)

**Task**: AZ-3656_pipeline_small_upload_batch
**Name**: Small-file batch pipeline
**Description**: Staged small uploads and the idempotent `commits:batch` publish many files in one transaction, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3801_crate_skeleton_accept_loop, AZ-3803_body_limits_timeouts, AZ-3829_tree_transfer_feed_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3737_transfer_stage_small, AZ-3754_transfer_commit_batch, AZ-3756_transfer_janitors_reservations, AZ-3709_tree_publish_core, AZ-3711_tree_quota_gc_deltas, AZ-3750_quota_ledger, AZ-3726_blob_writer, AZ-3727_group_commit_barrier, AZ-3729_blob_registry_gc_queue, AZ-3775_feed_stamp, AZ-3690_account_admin_and_guard, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3656
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p04

## Problem

The batch path depends on 03's receipt and blob effects being handed to 02's `publish` with per-entry savepoints, on 07a and 05 being called inside that transaction, and on the receipt and staged-expiry janitors being scheduled. Those joins exist only in the composition root. No planned task proves through the real routes that a batch is applied once, replayed unchanged, and fails per entry.

## Outcome

- Files up to 2 MiB staged over HTTP/2 and published by one `commits:batch` call through the real `serve` process appear together with one feed advance.
- Every component operation of flow F5 has a production caller, and the receipt and staged-expiry janitors run from the scheduler.
- At least one integration test runs F5 through the real binaries over HTTP.

## Scope

### Included
- Flow order and seams (`system-flows.md` F5 steps 1-5):
  1. Staged-upload route (client staged key, `X-Blake3`) → `UploadService::stage_small` → encrypt, write, sync → staged row, quota reserved.
  2. `POST /api/v1/commits:batch` with commit id C → `CommitService::commit_batch`.
  3. `ReceiptEffect` looks up C (replay returns the stored response) → `TreeCommit::publish` with one savepoint per entry (name rules, F8 checks, revision, `QuotaLedger::adjust_used` and `release_reservation`) → receipt stored → `FeedStamp::stamp` once for the batch → COMMIT → `FeedNotifier::notify`.
  4. Scheduler (12) → `ReceiptJanitor::run_cycle` and staged expiry in `SessionReaper::run_cycle` (03).
- The end-to-end integration test of the steps above.

### Excluded
- Large files (E-22/p03), conflict rules beyond per-entry outcomes (E-22/p07), client batching and pacing (E-23/c10, E-26/c09).

## Acceptance Criteria

AC-1..AC-6 are local to this task. No interaction risk of `interaction-risks.md` is owned here (AC-69 is in E-22/p06, AC-74 in E-22/p07).

### Wiring and flow criteria

**AC-1: Batch round trip**
Given a signed-in device and 200 files between 0 and 2 MiB
When it stages them in parallel over HTTP/2 and sends one `commits:batch`
Then 200 entries are committed, the listing shows all of them, and the account feed counter advanced by exactly one

**AC-2: Replay after a lost response**
Given a batch committed with commit id C whose response the client never received
When the client retries with the same C and the same entries
Then it gets the stored response unchanged, with no duplicate node and no conflict copy

**AC-3: Entries fail independently**
Given a batch with one invalid name and one expired staged key among valid entries
When it is committed
Then those two entries carry typed per-entry errors and every other entry is committed

**AC-4: Edge sizes**
Given a zero-byte file and a batch above the configured maximum
When they are staged and committed
Then the zero-byte file commits through the staged path, and the oversized batch (more entries than `batch_max_entries`) gets 422 `batch_too_large`

**AC-5: Staged expiry from the scheduler**
Given a staged file never committed and the test clock past its TTL
When the scheduler cycle runs
Then the staged row is gone and its reserved bytes are released

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `UploadService::stage_small`, `CommitService::commit_batch`, `TreeCommit::publish` with the receipt and blob effects, `FeedStamp::stamp`, and scheduler registrations of `ReceiptJanitor` and `SessionReaper`

## Non-Functional Requirements

**Performance**
- 10,000 small files in 60 s at 20 ms RTT (AC-40) is measured in E-31/c42; this task's test uses 200 files.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named operation and janitor registration has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 200 files up to 2 MiB | Parallel staging, one batch | 200 committed, counter +1 | AC-40 (scaled) |
| AC-2 | Committed batch C, response dropped | Retry with C | Stored response, no duplicates | |
| AC-3 | Batch with bad name and expired staged key | Commit | Two typed entry errors, rest committed | |
| AC-4 | Zero-byte file; oversized batch | Stage and commit | Zero-byte committed; 422 `batch_too_large` | |
| AC-5 | Uncommitted staged file, test clock | Scheduler cycle | Row gone, reservation released | |

## Constraints

- Owner: component 12 (`src/server/`): composition-root bindings and scheduler registrations for this flow, and its end-to-end tests among 12's integration tests. No SQL, blob I/O or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume over HTTP; no sunnycloud component is faked.
- Downstream: E-31/c23 (FT-P-04) and E-31/c32 (FT-N-38) should depend on this task.

## Risks & Mitigation

**Risk 1: Counter advanced per entry**
- *Risk*: the stamp runs inside each savepoint instead of once for the batch.
- *Mitigation*: AC-1 checks the counter delta of exactly one.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `NothingToSend` after a lost commit for a new file is still a spike (E-14/c02, E-23/c01) | E-14 | open | Low |
