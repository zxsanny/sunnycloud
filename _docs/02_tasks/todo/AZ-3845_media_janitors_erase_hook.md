# Media janitors, blob-owner cleanup and the account erase hook

**Task**: AZ-3845_media_janitors_erase_hook
**Name**: Media maintenance and erase hook
**Description**: Erase an account's jobs and derived files in bounded batches, remove derived rows whose content is gone, reap and purge old jobs and stale worker rows, and delete the derived row that owns a blob the GC removes
**Complexity**: 3 points
**Dependencies**: AZ-3840_job_queue, AZ-3842_derived_store_accept_input, AZ-3729_blob_registry_gc_queue, AZ-3706_tree_skeleton, AZ-3768_contracts_part1
**Component**: 08 media_jobs
**Tracker**: AZ-3845
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md` — `GcQueue` and the obligations of a `BlobOwnerCleanup` implementer (E-10/c09).
- `_docs/02_document/contracts/07_admin/deletion_purger.md` — how `erase_batch` is driven (E-17/c06).
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `Janitor`, `AccountLifecycleHook`, `BlobOwnerCleanup` (E-00/c03).

## Problem

Deleting an account must also remove its thumbnails and transcodes (AC-27). Derived files whose content no longer has any revision are waste and must go without retention; terminal jobs and stale worker rows must not pile up; and when 04's GC removes a derived blob, its row must go in the same transaction.

## Outcome

- `AccountLifecycleHook`: `initialize` does nothing; `erase_batch(account, limit)` deletes up to `limit` of the account's jobs first, then its derived rows, queuing each derived blob with `Due::Now`, and returns the remaining count; it is safe to call again after 0.
- `JobReaper` (`Janitor`): reap expired exhausted jobs, purge terminal jobs older than `job_terminal_ttl`, delete `worker_status` rows not seen for `job_terminal_ttl`.
- `DerivedJanitor` (`Janitor`): delete derived rows whose (account, content hash) has no remaining revision and queue their blobs with `Due::Now`.
- `BlobOwnerCleanup::remove_owner_rows(tx, blob_id)`: delete the derived row that owns the blob inside the caller's transaction; no row is success.

## Scope

### Included
- The hook, the two janitors and the cleanup, and the contract file `contracts/08_media_jobs/maintenance_ports.md`.

### Excluded
- Driving the hook (E-17/c06); scheduling the janitors and registering the lists (E-22/c03, E-22/c06); the GC itself (E-10/c10).

## Acceptance Criteria

**AC-1: Erase drains (AC-27)**
Given an account with 120 jobs in every status and 200 derived rows, and another account with media
When `erase_batch(account, 50)` is called until it reports 0, and once more
Then no call deletes more than 50 rows, the remaining count falls to 0, every derived blob of the account is queued with `Due::Now`, the extra call returns 0 without error, and the other account is untouched

**AC-2: Derived janitor removes only orphan content**
Given derived rows whose content has no remaining revision and others whose content still has one
When `DerivedJanitor` runs a cycle
Then only the first group is deleted and their blobs are queued with `Due::Now`

**AC-3: Job reaper**
Given terminal jobs older and newer than `job_terminal_ttl`, running jobs with expired leases and exhausted attempts, and `worker_status` rows seen recently and long ago
When `JobReaper` runs a cycle
Then old terminal rows and stale worker rows are deleted, recent ones stay, and the expired exhausted jobs become `dead`

**AC-4: Blob-owner cleanup**
Given a derived blob chosen by GC
When `remove_owner_rows` runs in a transaction that commits, in one that rolls back, and for a blob with no derived row
Then the owning derived row is gone after the commit, unchanged after the rollback, and the third call succeeds without change

## Non-Functional Requirements

**Reliability**
- Each janitor cycle is bounded and restart-safe by query alone; rows are claimed with `SKIP LOCKED` (`data_model.md` §12.1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Remaining count after mixed job and derived deletes | Jobs first, then derived; count correct |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 120 jobs, 200 derived | IT-08-15 erase | At most 50 per call; 0 remaining; `Due::Now` | FT-P-19, NFT-SEC-27 |
| AC-2 | Orphan and live content | IT-08-15 derived janitor | Only orphans removed | |
| AC-3 | Old and new terminal jobs, worker rows | IT-08-15 reaper | Old removed; dead marked | NFT-RES-23 |
| AC-4 | Derived blob; commit, rollback, none | IT-08-15 cleanup | As listed | |

## Constraints

- The cleanup works only inside the given transaction: no own transaction, no file I/O, no other port (04 contract obligations).

## Risks & Mitigation

**Risk 1: Janitor load on the API process (R12, R14)**
- *Risk*: purges compete with request traffic.
- *Mitigation*: batched deletes within a bounded cycle.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | - | - | - |

## Contract

This task produces `_docs/02_document/contracts/08_media_jobs/maintenance_ports.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
