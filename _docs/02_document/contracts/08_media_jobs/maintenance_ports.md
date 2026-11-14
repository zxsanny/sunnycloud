# Contract: media maintenance ports (erase hook, blob-owner cleanup, janitors)

**Component**: 08_media_jobs
**Producer task**: E-18/c10 — `_docs/02_tasks/todo/AZ-3845_media_janitors_erase_hook.md`
**Consumer tasks**: E-17/c06 and E-17/c11 (purger drives the hook), E-10/c10 (GC calls the cleanup), E-22/c03 (registers hook, cleanup and janitors), E-22/c06 (schedules the janitors); pipelines AZ-3663, AZ-3665
**Version**: 1.1.0
**Status**: active
**Last Updated**: 2026-11-14

## Purpose

What 08 does when an account is erased, when 04's GC removes a derived blob, and on its two maintenance cycles. Trait signatures are in `contracts` part 1; the driving rules of `erase_batch` are `_docs/02_document/contracts/07_admin/deletion_purger.md`; the cleanup obligations are `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md`.

## Shape

Types of crate `media-api`, registered by 12:

| Type | Ports | Constructor |
|------|-------|-------------|
| `MediaLifecycle` | `AccountLifecycleHook`, `BlobOwnerCleanup` | `new(db, gc: Arc<dyn GcQueue>)` |
| `JobReaper` | `Janitor`, name `job_reaper` | `new(db, clock, queue: Arc<JobQueue>, &MediaConfig)` |
| `DerivedJanitor` | `Janitor`, name `derived_janitor` | `new(db, clock, gc: Arc<dyn GcQueue>)` |

| Port and method | Behaviour | Errors | Transaction |
|-----------------|-----------|--------|-------------|
| `AccountLifecycleHook::initialize(tx, account)` | nothing | none | caller's |
| `AccountLifecycleHook::erase_batch(account, limit)` | delete up to `limit` rows of the account: up to `limit` jobs (any status) first, then with what is left of `limit` its derived rows, queuing each derived blob with `GcQueue` `Due::Now` (reason `AccountDeleted`) before its row is deleted; return `EraseProgress{remaining}` = the account's jobs plus derived rows left after the call | `Unexpected` | own short account-scoped transaction per call |
| `BlobOwnerCleanup::remove_owner_rows(tx, blob_id)` | delete the derived row whose `blob_id` is the blob (unique across accounts); no row is success | `Unexpected` | caller's only; no file I/O, no other port |
| `JobReaper::run_cycle(now)` | `JobQueue::reap(now)`: running jobs with an expired lease and exhausted attempts become `dead`; `JobQueue::purge_terminal(now − job_terminal_ttl)`: terminal jobs finished before the cut-off are deleted in batches of 1000; `worker_status` rows with `seen_at` before the same cut-off are deleted. `CycleStats.processed` = jobs marked dead + job rows deleted + worker rows deleted | `Unexpected` | short system-scope transactions |
| `DerivedJanitor::run_cycle(now)` | batches of 100 derived rows whose (account, content hash) has no revision row left in that account, across accounts: each blob queued `Due::Now` (reason `DerivedRemoved`), then the rows deleted, in one transaction per batch, until a batch finds fewer than 100. `now` plays no part (no retention). `processed` = derived rows deleted | `Unexpected` | one system-scope transaction per batch |

A cycle that handled rows logs one INFO line with its duration (no ids, no counts). Scheduler intervals: `job_reap_interval` (1 min), `derived_janitor_interval` (1 h).

## Invariants

- `erase_batch` never deletes more than `limit` rows in one call (jobs plus derived rows), touches only the given account, and returns 0 again when called after it reported 0 or for an unknown account. A row locked by a concurrent claim, report or janitor is skipped (`SKIP LOCKED`) and stays in `remaining` until a later call.
- `remaining` counts 08's rows only; the queued derived blobs are counted by the GC queue (`pending_for_account`), which the tree hook reports.
- Every derived blob that loses its row is queued for GC in the same transaction as the row delete; derived blobs have no retention.
- `remove_owner_rows` never opens its own transaction; the caller's rollback undoes it.
- Lock order: a derived row first (`FOR UPDATE SKIP LOCKED`), then its blob row through the queue, in the erase and in the janitor alike. The sweeper locks only queued blobs, and a derived blob is queued in the transaction that deletes its row, so the sweeper never waits on a row these hold.
- Janitors are restart-safe by query alone, claim rows with `SKIP LOCKED` and need no leader.

## Non-Goals

- Not covered: file removal (04 GC), the order of hooks (07, 12), scheduling the janitors (12).

## Versioning Rules

- **Breaking changes** (what `remaining` counts, a cleanup that opens its own transaction, a janitor removing live content) require a new major version and notice to E-10, E-17 and E-22.
- **Non-breaking additions** (a new janitor statistic) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| erase | 120 jobs in every status, 200 derived, limit 50; another account with media | 7 calls; at most 50 rows per call, jobs first; 0 remaining; every derived blob queued `account_deleted`, `Due::Now`; other account untouched | IT-08-15, `erase_hook::erase_batch_drains_…` |
| erase-repeat | call after 0; unknown account | 0, no error | same test |
| derived-janitor | 153 orphans over two accounts (one with another account's content), 8 live rows | 153 removed and queued `derived_removed` due now; live rows stay; a second cycle removes 0 | IT-08-15, `janitors::the_derived_janitor_…` |
| reaper | old and new terminal jobs, an expired exhausted job, old and fresh worker rows | old rows removed, recent kept, the job dead, `processed` 4 | IT-08-15, `janitors::the_job_reaper_…` |
| cleanup | real GC sweep of a queued derived blob; a rolled-back call; a blob with no row | row gone with the blob; row kept; success | IT-08-15, `erase_hook::the_gc_sweeper_…`, `erase_hook::a_rolled_back_cleanup_…` |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.1.0 | 2026-11-14 | As built: type names and constructors, batch sizes, `processed` per janitor, lock order, `remaining` counts 08's rows only | AZ-3845 |
