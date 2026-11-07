# Batch Report

**Batch**: 21
**Tasks**: AZ-3753_quota_concurrency_suite, AZ-3726_blob_writer, AZ-3776_change_feed_read, AZ-3817_admin_user_management
**Date**: 2026-11-07

Four workers on four disjoint crates (`07a_quota`, `04_blobstore`, `05_sync_feed`, `07_admin`) against one PostgreSQL 18 test container (near its 400-connection limit with four workers; the quota worker cut its pools from 24 to 8). No worker left its `Owns:` paths. Parent edit: `src/components/07_admin/tests/user_list.rs` (the workspace `silent_catch` scan flagged `let _ = (account, ...)`; replaced by `field: _` patterns on a reference, which keeps the compile-time check that `AdminUserView` has no extra field). AZ-3729 deferred (same crate as AZ-3726), AZ-3893 deferred (Windows only).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3753_quota_concurrency_suite | Done | `sunnycloud-quota` tests only: `concurrency.rs`, `support/workload.rs` | 4 new (60 in the crate); no production defect found; 5 stable runs and 18 runs at 6 copies | Items 1-2 |
| AZ-3726_blob_writer | Implemented; 4 privileged rows NOT-RUN(C-priv) | `sunnycloud-blobstore`: `FileBlobWriter` (`writer/mod.rs`, `block.rs`, `complete.rs`, `volume.rs`); new deps `rustix`, `bytes`, `uuid` v4, dev `futures-core` | 30 new (157 in the crate) | Items 3-7 |
| AZ-3776_change_feed_read | Done; PT-05-2 NOT-RUN(HW-REF) | `sunnycloud-sync-feed`: `PgFeedReader` (`read_repo.rs`, one statement per page), `cursor.rs`, errors | 16 new (45 passed, 2 ignored in the feed binary) | Items 8-10 |
| AZ-3817_admin_user_management | Done | `sunnycloud-admin`: `list_users`, `update_user`, `set_quota`, actor check, `AdminCallError` | 47 | Items 11-14 |

## Code Review Verdict: PASS_WITH_WARNINGS

10 findings, none blocking: `plan()` keeps every planned size in an unbounded in-memory map; `set_quota` commits the limit and writes its audit row in two transactions; a cancelled `write_complete` leaves an orphan file; the free-space floor is checked, not reserved, so concurrent creates can breach it; a feed page is unbounded when one `seq` batch is large; `AdminCallError` duplicates `contracts::AdminError`; offset paging skips rows when the page cap bites; unknown account is detected through the quota row; every block write reopens the file and unwraps the key; the four privileged loop-volume rows have not run.

CI-parity: PASS after one fix. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All RC 0 except the first workspace test run, which failed `sc_error --test silent_catch` (2 tests, one source line in 07_admin); fixed, `silent_catch` 6 of 6, fmt, clippy and the admin tests re-run green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 1906 (workspace, default features, real PostgreSQL 18)
- Passed: 1901 (1899 in the full run + the 2 `silent_catch` tests after the fix)
- Failed: 0
- Skipped/ignored: 5 (3 as in earlier batches, PT-05-1, PT-05-2)

## Owner decisions

None needed. No allowlist line was added: the new code has no `run_system` call.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | A single quota row cannot deadlock, so the deadlock retry in the workload never fires; `pg_stat_statements` is absent, so "no recompute ran" is checked through `Db::stats()` | AZ-3753 | Keep | clear |
| 2 | The delete targets of the workload depend on which commits succeeded in earlier waves | AZ-3753 | Final values are checked against the actual ledger | unclear |
| 3 | `create_file` has no size and `BlobPlan` carries none; the writer remembers sizes in an in-memory map | AZ-3726 contract | Add `plaintext_size` to `BlobPlan` or `create_file` (contracts) | unclear |
| 4 | `FreeSpaceFloor` vs `NoSpace` when both apply: `available < needed` is `NoSpace`, else `available - needed < reserve` is `FreeSpaceFloor` | AZ-3726 AC-3/AC-4 | Loop-volume owner checks the 64 MiB setup | unclear |
| 5 | A wrong-length `write_block` is `SizeMismatch`; a bad size is `OutOfRange`; a bad header is `Corrupt` | AZ-3726 | Keep | unclear |
| 6 | Four dependencies were added without asking: `rustix` (statvfs, fallocate), `bytes`, `uuid` v4, dev `futures-core`. Pinning, `cargo deny` and `cargo test` accept them | AZ-3726 | Owner may veto | unclear |
| 7 | Four loop-volume rows NOT-RUN(C-priv); the spec says AZ-3726 is not done until they run. Jira stays In Testing; leftover `_docs/_process_leftovers/2026-11-07_az-3726-loop-volume-rows.md` | AZ-3726 | Run on a privileged runner | unclear |
| 8 | Reason order when a cursor breaks several rules: epoch, horizon, ahead | AZ-3776 flagged concern | Epic owner | unclear |
| 9 | PT-05-2 p95 about 99 ms in a debug smoke run at 100,000 nodes (statement about 1 ms in the database); plan check ran with an empty `revisions` table | AZ-3776 AC-8 | Run on REF-SRV with the 1M template | unclear |
| 10 | `feed_page_limit_max` still has no value; a page can exceed `limit` by the rest of its last `seq` batch | AZ-3776 | Epic owner; cap the batch in 02 | unclear |
| 11 | `contracts::AdminError` has no `Forbidden`; 07 added `AdminCallError` | AZ-3817 AC-6 | Add the variant in contracts, then delete `AdminCallError` | unclear |
| 12 | Tests use SQL-backed doubles for 01 and 07a, because `crate_graph` forbids component edges even in dev-dependencies | AZ-3817 | Keep; end-to-end in `src/server` | clear |
| 13 | AC-2 "authenticator rejects all 3 credentials" is asserted as `revoked_at` flags set in the same transaction; the authenticator side is 01's | AZ-3817 AC-2 | Cover in IT-01-15 | clear |
| 14 | `admin_list_page_size` has no default and no defined role; `AdminServiceImpl` does not implement the `AdminService` trait yet (needs `health` AZ-3821 and `Forbidden`) | AZ-3817 | Epic owner | unclear |
| 15 | Items open from batches 18-20 stay open (see their reports) | batches 18-20 | | unclear |

## Commit

`[AZ-3753] [AZ-3726] [AZ-3776] [AZ-3817] quota concurrency suite, blob writer, change feed read, admin user management` — Loop: 1

## Next Batch

Recompute from `_docs/02_tasks/_dependencies_table.md` at the start of batch 22. Likely ready: AZ-3728 blob reader (needs AZ-3726, AZ-3729), AZ-3729 blob registry, the feed tasks that depend on AZ-3776, the media queue tasks that depend on AZ-3839, plus the spikes. 293 tasks left.
