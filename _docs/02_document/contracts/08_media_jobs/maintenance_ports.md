# Contract: media maintenance ports (erase hook, blob-owner cleanup, janitors)

**Component**: 08_media_jobs
**Producer task**: E-18/c10 — `_docs/02_tasks/todo/B05-21_media_janitors_erase_hook.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-17/c06 and E-17/c11 (purger drives the hook), E-10/c10 (GC calls the cleanup), E-22/c03 (registers hook, cleanup and janitors), E-22/c06 (schedules the janitors); pipelines AZ-3663, AZ-3665
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

What 08 does when an account is erased, when 04's GC removes a derived blob, and on its two maintenance cycles. Trait signatures are in `contracts` part 1; the driving rules of `erase_batch` are `_docs/02_document/contracts/07_admin/deletion_purger.md`; the cleanup obligations are `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md`.

## Shape

| Port and method | Behaviour | Errors | Transaction |
|-----------------|-----------|--------|-------------|
| `AccountLifecycleHook::initialize(tx, account)` | nothing | none | caller's |
| `AccountLifecycleHook::erase_batch(account, limit)` | delete up to `limit` rows of the account: its jobs first, then its derived rows, queuing each derived blob with `GcQueue` `Due::Now` (reason `AccountDeleted`); return `EraseProgress{remaining}` = jobs plus derived rows still present | `Unexpected` | own short transaction per call |
| `BlobOwnerCleanup::remove_owner_rows(tx, blob_id)` | delete the derived row that owns the blob; no row is success | `Unexpected` | caller's only; no file I/O, no other port |
| `JobReaper` (`Janitor`) `run_cycle(now)` | mark running jobs with an expired lease and exhausted attempts `dead`; delete terminal jobs finished before now − `job_terminal_ttl` in batches; delete `worker_status` rows not seen for `job_terminal_ttl` | `Unexpected` | short batches, system scope |
| `DerivedJanitor` (`Janitor`) `run_cycle(now)` | delete derived rows whose (account, content hash) has no remaining revision and queue their blobs with `Due::Now` (reason `DerivedRemoved`) | `Unexpected` | short batches |

Scheduler intervals: `job_reap_interval` (1 min), `derived_janitor_interval` (1 h).

## Invariants

- `erase_batch` never deletes more than `limit` rows in one call, touches only the given account, and returns 0 again when called after it reported 0 or for an unknown account.
- Every derived blob that loses its row is queued for GC in the same transaction as the row delete; derived blobs have no retention.
- `remove_owner_rows` never opens its own transaction; the caller's rollback undoes it.
- Janitors are restart-safe by query alone, claim rows with `SKIP LOCKED` and need no leader.

## Non-Goals

- Not covered: file removal (04 GC), the order of hooks (07, 12).

## Versioning Rules

- **Breaking changes** (what `remaining` counts, a cleanup that opens its own transaction, a janitor removing live content) require a new major version and notice to E-10, E-17 and E-22.
- **Non-breaking additions** (a new janitor statistic) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| erase | 120 jobs, 200 derived, limit 50 | at most 50 per call; 0 remaining; blobs `Due::Now` | IT-08-15 |
| erase-repeat | call after 0; unknown account | 0, no error | |
| derived-janitor | orphan and live content | only orphans removed | IT-08-15 |
| reaper | old and new terminal jobs, stale worker rows | old removed | IT-08-15 |
| cleanup | commit, rollback, no row | removed, kept, success | IT-08-15 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
