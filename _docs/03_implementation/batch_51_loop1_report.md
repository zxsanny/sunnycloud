# Batch Report

**Batch**: 51
**Tasks**: AZ-4238_mobile_code_refusal (done), AZ-3717_tree_lifecycle_blob_cleanup (done), AZ-3781_feed_via_tree_integration (done), AZ-3841_job_planner_enqueue_effect (done); AZ-3925_vault_erase_hook closed (its AC-4 ran here)
**Date**: 2026-11-12

Four parallel workers on disjoint crates (`sync-core` + 11's identity test, `02_tree`, `05_sync_feed` tests, `08_media_jobs`). Before they started, the parent added the dev-dependencies they needed (05 and 08 on tree and quota, 08 on the feed, tree on the vault service), so only the parent touched `Cargo.lock` (path entries only, no new external package). The parent wrote the contract changes, reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-4238_mobile_code_refusal | Done | AC-1: on the mobile code exchange, a 400 `invalid_request` whose `problems` name the field `code` becomes `SyncError::InvalidCode`, through a narrow route hook (`send_anonymous_reading`) read before the wire table; a 400 naming `body`, and the same answer on other routes, keep their mapping. AC-2: the fixtures answer the server's shape (`field_refused`). AC-3: the real `HttpSyncClient` against the full 11 stack gets `InvalidCode` for a reused code. The dead `invalid_code` mapping is gone |
| AZ-3717_tree_lifecycle_blob_cleanup | Done | `PgTreeLifecycle` (new, beside `PgVaultLifecycle`): `initialize` inserts the root (a second root refused by `nodes_root_key`); `erase_batch` in one short retried transaction without the advisory lock queues each revision's blob (`AccountDeleted`, `Due::Now`, blob row locked first, the sweeper's order) before deleting it, then leaves level by level, root last; `remaining` = nodes + revisions + queued blobs; `remove_owner_rows` deletes the revision of a blob in any state. AC-1..AC-4 pass with the real 04 `GcQueue` and `GcSweeper` (1,000 nodes, 600 revisions, a restart between batches) |
| AZ-3925_vault_erase_hook | Done | AC-4: the 20 hook then the 02 hook erase an account with a vault rooted in its tree, both reach `remaining = 0`, no key failure; the reverse order is refused by `vaults.root_node_id` and rolls back |
| AZ-3781_feed_via_tree_integration | Done | Through the real `publish`: AC-69 (50 concurrent creates, then 50 updates; each node once in its latest state, seq 1..50 and 51..100, final cursor = head), AC-1 (stamp, COMMIT, notify in that order; the held reader wakes after the notify only; a COMMIT refused after the stamp by a test-installed deferred trigger leaves no gap and wakes nobody), AC-2 / AT-05-1 (B parked at head wakes 1.4–2.2 ms after A's notify, bound 50 ms). 8 of 8 runs green |
| AZ-3841_job_planner_enqueue_effect | Done | `JobPlanner` (pure plan, priority, count limit) and `MediaEnqueueEffect` (`on_entry` in the commit transaction: one `derived` probe, one queued count skipped with the import hint, one insert per due job through AZ-3840's `enqueue_in_tx`). AC-1..AC-5 pass; AC-4 through the real `publish` (failed entry and failed batch leave no job); AC-5 counted from sqlx statement events (2 probes + 1 insert; EXPLAIN names `derived_pkey` and `jobs_queued_idx`) |

## Contract and document changes (parent)

`media_enqueue_effect.md` 1.1.0 (implemented shape; open point 1 closed by the AZ-3709 decision). `sync_api.md`: the exchange row and open point 3 state the server's refusal (400 `invalid_request`, field `code`). `data_model.md` §12.2: hook order 06, 20, 02, 03, 08, then 05 and 07a; erase batches neither stamp the feed nor adjust the quota (owner decision 2026-11-12). Flagged concerns of AZ-3717 and AZ-3781 marked resolved.

## Owner decisions 2026-11-12

1. Erase does not stamp the feed or adjust the quota: `data_model.md` §12.2 corrected (it also listed 20 after 02, against the 2026-11-05 order).
2. An index on `nodes(account_id)` was approved on the worker's finding that the erase scans `nodes`. The parent checked before writing the migration: `nodes_feed_idx (account_id, seq)` (05's migration) already leads with `account_id`, and EXPLAIN on the migrated template shows both erase statements (leaf delete and count) on a bitmap scan of `nodes_feed_idx`, the anti-join on `nodes_parent_idx`. No migration was added: a second index would only cost writes and space.

## Code Review Verdict: PASS_WITH_WARNINGS

Fixed by the parent:
- The feed test's notifier swallowed the "outside a scope" error in an empty `Err(_)` arm, which the workspace silent-discard gate (`sc_error` `silent_catch`) refuses. Every publish of that test binary now runs in the scope, and the notifier sets it unconditionally.
- 08's statement test reached `tracing` through `sc_observability::__private`; 08 now has `tracing` as a dev-dependency (already in the lockfile).
- 05 and 08 did not use the `sunnycloud-blobstore` dev-dependency the parent had added; removed.

Warnings (recorded, not changed):
- `erase_batch` retries a deadlock only when the error is a `DbError`; a deadlock inside `GcQueue::schedule_in_tx` comes back as a `GcQueueError` and fails the batch, which the purger repeats on its next tick.
- A blob the sweeper removes while the erase queues it fails that erase batch with `Unexpected("no blob row")`; the next tick succeeds.

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, wasm build and the four check scripts exit 0. Workspace tests on PostgreSQL 18: 3039 passed, 3 failed, 19 ignored. Two failures were the silent-discard gate on the feed test (fixed above; `silent_catch` 6/6 and the feed tests 3/3 after the fix). The third, `sunnycloud-quota` `ledger_locks::second_reserve_waits_for_a_commit_and_both_are_counted`, measured a 399.97 ms wait against its 400 ms floor under the full parallel run; the crate is not changed by this batch and the test passes 3 of 3 alone. Recorded as a timing-flaky test.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | The quota lock-wait test compares a measured wait to the exact hold time and can fall short by microseconds under load | 07a | Follow-up: measure from before the first lock, or allow timer slack | clear |
| 2 | `pg_stat_statements` is available in the test container but not preloaded, so statement counts use sqlx's tracing events | AZ-3841 AC-5 | As implemented | clear |
| 3 | A COMMIT refused after the stamp is produced by a deferred constraint trigger the test installs, the only real way through `publish` (the stamp is its last statement) | AZ-3781 AC-1 | Accepted | clear |
| 4 | The checker matches the field `code` only, not the problem text `invalid` | AZ-4238 | As implemented; the server sends only `invalid` for that field | unclear |

## Commit

`[AZ-4238] [AZ-3717] [AZ-3781] [AZ-3841] [AZ-3925] Tree erase hook, feed via tree, media enqueue effect` — Loop: 1

## Next Batch

Recompute the ready tasks; AZ-3714 (cascade purge janitors, 5 points, unblocks AZ-3710) needs the complexity-budget choice.
