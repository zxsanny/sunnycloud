# Quota deltas and GC scheduling in publish, delete and cascade

**Task**: AZ-3711_tree_quota_gc_deltas
**Name**: Tree quota and GC deltas
**Description**: Apply the exact usage delta of every tree change through `QuotaLedger::adjust_used` and queue each unreferenced blob with `GcQueue::schedule_in_tx` in the same transaction, so stored usage always equals the sum of live current revisions
**Complexity**: 3 points
**Dependencies**: AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3710_tree_conflict_rules, AZ-3714_tree_cascade_purge_janitors, AZ-3716_tree_revision_lookups, AZ-3750_quota_ledger, AZ-3751_quota_admin_lifecycle, AZ-3752_quota_recompute, AZ-3729_blob_registry_gc_queue
**Component**: 02_tree
**Tracker**: AZ-3711
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — commit contract; this task fixes the per-outcome usage deltas.

## Problem

Usage is stored in 07a, but only the tree knows how much a change adds or frees. If a replace, a stale-base commit, a revival, a delete or a cascade batch applies the wrong delta, or applies it outside the transaction that changes the rows, usage drifts above the quota or below the real sum (AC-74, R7). The same transactions must queue every blob that stops being current, or deleted content is never collected (AC-30).

## Outcome

- Each change applies one delta in the same transaction: new +size, replace +new−old, stale base +new, revive +new, delete −current size, cascade −sum per batch.
- A replaced blob is queued `Replaced` and a deleted file's blob `deleted`, both due after retention, in the transaction that unreferences them.
- After 1,000 concurrent commits and deletes, stored usage equals `LiveUsageSource::live_bytes` and never exceeds the quota.

## Scope

### Included
- Deltas for `Updated`/`Replaced` (+new−old), `ConflictCopied` (+new; the superseded revision still counts in its copy), `Revived` (+new), `NoChange` (0), `delete_node` (−current size, once), and each cascade batch (−sum of the batch).
- `GcQueue::schedule_in_tx` for the replaced blob (reason `replaced`, due now + retention) in `publish`, and for the blobs of deleted and cascaded files (reason `deleted`, due now + retention).
- The failed-entry rule: a savepoint rollback also undoes the entry's delta and reservation release.

### Excluded
- The `+size` delta of `Created` and the reservation release mechanism (E-13/c05); erase-time queueing with `Due::Now` (E-13/c13); restore deltas (E-13/c11).
- Quota limits and refusals (E-11).

## Acceptance Criteria

**AC-1: Publish deltas and replaced-blob queueing (AC-30, AC-74)**
Given files committed through `publish`
When a file is updated on its current base, published stale-base, and revived, and the same content is published again
Then usage changes by +new−old, by +new (after a stale-base r3 over r2, usage equals size(r3) + size(r2)), by +new and by 0, and the replaced blob has a `blob_gc` row with reason `replaced` and `due_at` = now + retention

**AC-2: Delete delta applied once (AC-74)**
Given file f of 10 MiB at current revision r2
When f is deleted with stale base r1, then with base r2 twice
Then usage is unchanged after the stale call, falls by exactly 10 MiB after the first correct call and does not change on the repeat, and f's blob is queued once with reason `deleted`

**AC-3: Cascade batches release exact bytes**
Given folder F holding 100 files of 1 KiB, `cascade_batch_size = 20`
When F is deleted and `cascade_batch` runs until it returns 0
Then usage falls by exactly 100 KiB across the 5 batches, `QuotaAdmin::view.used` equals `LiveUsageSource::live_bytes` after every batch, and `GcQueue::pending_for_account` grows by 101 in total with f from AC-2

**AC-4: Exact usage under concurrency (AC-74)**
Given quota 64 MiB and a sampler reading `QuotaAdmin::view` every 5 ms
When 1,000 concurrent operations run on one account (600 reserve + publish of 1 KiB to 1 MiB with some replaces and stale bases, 300 `delete_node`, 100 cascade batches)
Then sampled usage never exceeds 64 MiB, at the end `view.used` equals `live_bytes` equals the result of `QuotaAdmin::recompute(live_bytes, reserved_bytes)`, reserved bytes equal the sum of open reservations, and refused reservations never reach `publish`

**AC-5: Failed entries leave usage untouched**
Given an entry whose `NewRevision.reserved_bytes` belongs to a reaped session, so the effect fails with `Entry`
When it is published inside the AC-4 workload
Then used and reserved bytes are unchanged for that entry

## Non-Functional Requirements

**Reliability**
- Deltas are aggregated per batch where possible so the quota row is touched once per transaction (R07 mitigation).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Delta per outcome table | +size, +new−old, +new, +new, 0 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Committed files | Update, stale, revive, same hash (IT-02-5, IT-02-6, IT-02-8) | Deltas as listed; `replaced` row due after retention | FT-P-16 |
| AC-2 | f 10 MiB at r2 | Stale, correct, repeat delete (IT-02-4) | 0, −10 MiB, 0; one `deleted` row | |
| AC-3 | F with 100 x 1 KiB | Cascade loop (IT-02-4) | −100 KiB; used = live_bytes each batch; +101 queued | |
| AC-4 | Quota 64 MiB, sampler | 1,000 mixed operations (IT-02-15) | Never above quota; end values equal | NFT-RES-38 |
| AC-5 | Reaped-session entry | Publish in workload (IT-02-15 variant) | Used and reserved unchanged | |

## Constraints

- Real E-11 `QuotaLedger`/`QuotaAdmin` and real E-10 `GcQueue` on one PostgreSQL; the end-to-end AC-74 run through the routes belongs to the pipeline AZ-3659, and the ledger's own concurrency suite to E-11/c05.

## Risks & Mitigation

**Risk 1: Delta outside the savepoint**
- *Risk*: a delta applied after the entry savepoint survives an entry failure.
- *Mitigation*: AC-5 checks a failing entry inside the concurrent workload.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 02 §5 names the GC reason only for replaced blobs; the `deleted` reason is taken from `data_model.md` §6.2 | E-13 owner | accepted-risk | Low |
