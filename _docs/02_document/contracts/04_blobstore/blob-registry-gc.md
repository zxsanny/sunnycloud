# Contract: blob-registry-gc

**Component**: 04_blobstore
**Producer task**: E-10/c09 — `_docs/02_tasks/todo/B02-09_blob_registry_gc_queue.md` (`BlobRegistry`, `BlobCatalog`, `GcQueue`); E-10/c10 — `B02-10_gc_sweeper.md` (`GcSweeper` and its calls into `BlobOwnerCleanup`) (both renamed to their AZ keys at Step 4)
**Consumer tasks**: E-13 tree (GC scheduling in publish, delete, cascade and purge; reinstate in restore; ownership check by the composite foreign key on `revisions`, not through `BlobCatalog` (AZ-3712, 2026-11-13); implements `BlobOwnerCleanup` in E-13/c13), E-14 transfer (register rows, `get_ref` for downloads, row removal by the session reaper), E-18 media_jobs (derived rows, `Due::Now` scheduling, implements `BlobOwnerCleanup` in E-18/c10), E-17 / E-13 erase hooks (`pending_for_account`), E-22 (`admin gc-run`, scheduler)
**Version**: 1.0.0
**Status**: draft (frozen when E-10/c10 is accepted)
**Last Updated**: 2026-11-04

## Purpose

The row lifecycle of a blob (`uploading` -> `committed` -> `gc_pending` -> removed) and the obligations of the components that own rows pointing at a blob. Trait signatures live in `contracts` (E-00/c05 for the 04 ports; `BlobOwnerCleanup` and `Janitor` in E-00/c03, `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`). Source: `components/04_blobstore/description.md` §3, `data_model.md` §6.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `BlobRegistry::register_in_tx` | `tx, WrittenBlob or BlobPlan, state uploading or committed` -> `()` | typed | async, caller's tx |
| `BlobRegistry::mark_committed_in_tx` | `tx, blob_id, size` -> `()` | typed | async, caller's tx |
| `BlobRegistry::reinstate_in_tx` | `tx, blob_id` -> `Reinstated \| BeyondRetention \| FileMissing` | typed | async, caller's tx |
| `BlobRegistry::remove_row_in_tx` | `tx, blob_id` -> `()` | typed | async, caller's tx |
| `BlobCatalog::get_ref` | `account_id, blob_id` -> `Option<BlobRef{blob_id, size, wrapped_key, master_key_id}>` | none | async |
| `GcQueue::schedule_in_tx` | `tx, blob_id, reason, Due::AfterRetention \| Due::Now` -> `()` | typed | async, caller's tx |
| `GcQueue::pending_for_account` | `account` -> `u64` | typed | async |
| `GcSweeper::run_cycle` (`Janitor`) | `now` -> `GcStats{removed, remaining_due, budget_spent}` | typed | async |
| Requires `BlobOwnerCleanup::remove_owner_rows` | `tx, blob_id` -> `()` | `Unexpected` | async, sweeper's tx |

`reason` is one of `replaced`, `deleted`, `account_deleted`, `derived_removed` (data model 6.2).

## Invariants

- Registry methods run inside the caller's transaction and open none. A rolled-back caller transaction leaves no row and no state change.
- `BlobRef` is defined by 04 only. `get_ref` is account-scoped: another account's blob id and an unknown id both return `None`. It is the only read of `blobs` on the request path.
- `schedule_in_tx` is the single writer of `blob_gc` and of `blobs.state = 'gc_pending'`; both change together or not at all. `due_at = now + retention` for `AfterRetention`, `due_at = now` for `Due::Now`. Scheduling a blob that is already queued keeps one row with the earliest `due_at`.
- `pending_for_account` counts that account's queued blobs that are not yet removed, and nothing of other accounts.
- `reinstate_in_tx` checks the file before its database statements: within retention it returns `Reinstated` (queue row removed, state back to `committed`); a row already removed after retention gives `BeyondRetention`; a row whose file is absent gives `FileMissing`.
- The sweeper never removes a blob before its `due_at`. It claims due rows in a short transaction by moving `due_at` forward by `gc_claim_lease`, so rows stay claimable if the process dies. It unlinks files outside any transaction. A file that is already missing is logged (WARN) and its rows are still removed.
- After the unlink, one short transaction calls `remove_owner_rows(tx, blob_id)` of **every** registered `BlobOwnerCleanup`, then `remove_row_in_tx`. A cycle repeats batches of `gc_batch` until nothing is due or `gc_cycle_budget` is spent.
- **Obligations of a `BlobOwnerCleanup` implementer (E-13, E-18)**: delete its row that references the blob whatever that row's state (02: the revision row, the FK clears `nodes.current_revision_id`; 08: the `derived` row); work only inside the given transaction (no own transaction, no file I/O, no call to another port); treat "no row" as success, because many blobs have no owner row (upload sessions, staged files, a derived blob already gone).

## Non-Goals

- Not covered: which blobs to schedule and when (02, 03, 08 decide); quota changes (07a); the orphan sweep (internal to 04).
- Not covered: the field list of `CycleStats` in `contracts` (open in part 1); `GcStats` is reported through it once that is fixed.

## Versioning Rules

- **Breaking changes** (state names, scheduling semantics, the cleanup call order or its transaction) require a new major version and notice to E-13, E-14 and E-18.
- **Non-breaking additions** (a new `reason`, a new optional stat) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| register-rollback | `register_in_tx(uploading)` then caller rollback | no row | IT-04-8 |
| catalog-foreign | B asks `get_ref(B, A.blob_id)` | `None` | IT-04-8, ST-04-5 |
| schedule-retention | retention 7 d, `AfterRetention` | `due_at = now + 7 d`, state `gc_pending` | IT-04-9 |
| schedule-duplicate | same blob scheduled twice | one row, earliest `due_at` | IT-04-9 |
| reinstate-states | within retention / removed / file absent | `Reinstated` / `BeyondRetention` / `FileMissing` | IT-04-8 |
| not-early | clock at `due_at - 1 ms` | 0 removed | IT-04-10 |
| cleanup-order | 250 due blobs, recording cleanup | cleanup called once per blob before its row removal | IT-04-10 |
| cleanup-no-row | blob of an aborted session (no revision row) | cleanup returns `Ok`, rows removed | |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
