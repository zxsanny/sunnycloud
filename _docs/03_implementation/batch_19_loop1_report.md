# Batch Report

**Batch**: 19
**Tasks**: AZ-3751_quota_admin_lifecycle, AZ-3727_group_commit_barrier, AZ-3774_sync_feed_skeleton
**Date**: 2026-11-07

Three workers on three disjoint crates (`07a_quota`, `04_blobstore`, `05_sync_feed` plus its migration) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: `NODE_INDEXES` in `src/components/02_tree/tests/tree/migration.rs` (the feed migration adds `nodes_feed_idx`, which the tree index test now lists as owned by 05), and the three owner-approved lines in `scripts/ci/run-system-allowlist.txt` (below). AZ-3839 was deferred (it shares the `sc_error` status table); AZ-3725 and AZ-3729 were deferred (same crate as AZ-3727).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3751_quota_admin_lifecycle | Done | `sunnycloud-quota`: `PgQuotaAdmin` (`view`, `views`, `set_limit`, `AccountLifecycleHook`), `admin_repo.rs`, `admin_error`/`lifecycle_error` | 13 new (54 in the crate) | Items 1-3 |
| AZ-3727_group_commit_barrier | Done | `sunnycloud-blobstore`: `SyncBarrier` (`register`, `await_durable`), real `fdatasync` group flush, `SyncHook` behind the non-default `sync-hook` feature | 18 new (barrier suite), XFS row NOT-RUN(C-priv) | Items 4-7 |
| AZ-3774_sync_feed_skeleton | Done | `sunnycloud-sync-feed`: `FeedConfig`, error converters; migration `20261106150000_create_feed_tables` (`account_feeds`, `nodes.seq`, `nodes_feed_idx`, RLS, DML grants) | 24 (real PostgreSQL 18) | Items 8-10 |

## Code Review Verdict: PASS_WITH_WARNINGS

8 findings, none blocking: token reuse corrupts the group waiter count; a registered but never awaited token leaks a group entry and holds the file open (both only through misuse of the API; the writer E-10/c06 must register once and await once, and must not drop a token unawaited); `views` runs in system scope with no 500-id cap; the 20 ms wall-clock test is load sensitive; `longpoll_auth_recheck` cross-rule not validated; no CHECK keeps feed counters non-negative; `erase_batch` ignores `limit`; sequential syncs inside a group.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All RC 0 after the allowlist lines (the first `check-db-gates` run failed on the three files below, as expected). Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 1745 (workspace, default features, real PostgreSQL 18)
- Passed: 1742
- Failed: 0
- Skipped: 3 (as in batch 17 and 18)

## Owner decision applied

The owner approved three `run-system-allowlist.txt` lines in this session: `src/components/07a_quota/src/admin_repo.rs` (`views` answers 500 accounts in one statement, which an account scope cannot see), `src/components/07a_quota/tests/views_statement_repo.rs` (proves the one-statement count; renamed from `views_statement.rs` because the gate accepts only `*_repo.rs` modules), `src/components/05_sync_feed/tests/feed/system_scope_repo.rs` (proves system scope sees every feed row).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `views` runs in system scope although the spec says "under the caller's scope"; `QuotaAdmin::views(&[Uuid])` has no scope argument | AZ-3751 Constraints | Keep; caller (07 with `Scope::Admin`) authorizes. Enforce the 500-id cap or refuse in the admin task | unclear |
| 2 | `QuotaAdmin` trait not implemented (no `recompute` yet); methods are inherent on `PgQuotaAdmin` | AZ-3751 Excluded | E-11/c04 adds `recompute` and the trait impl | clear |
| 3 | Test server has no `pg_stat_statements`; the one-statement test counts sqlx events and `Db::stats` units | AZ-3751 AC-3 | Enable `pg_stat_statements` on the dev container if the stricter form is wanted | unclear |
| 4 | After a failed `fdatasync` Linux may mark pages clean; a retry of the same bytes could report success | AZ-3727 AC-2 | Writer (E-10/c06) must rewrite the block, not re-await, and must not ack on a retry alone | clear, note for c06 |
| 5 | `write_block` must register the file after `pwrite` completes and before it returns the token; one register, one await per token | AZ-3727 contract | Carry into E-10/c06; consider releasing a group entry when a token drops unawaited | clear, note for c06 |
| 6 | XFS loop-volume timing row needs a privileged runner | AZ-3727 AC-3 | NOT-RUN(C-priv) | clear |
| 7 | 20 ms wall-clock test is load sensitive | AZ-3727 AC-4 | Keep strict per spec; retry in CI if it flakes | unclear |
| 8 | `sc_config` binds the `longpoll_wait_max` range only in production; `FeedConfig::validate` checks it everywhere | AZ-3774 AC-3 | Keep | clear |
| 9 | `longpoll_auth_recheck` cross-rule (below 5 s minus session cache TTL) and `feed_page_limit_max` default stay open | AZ-3774 flagged concerns | Epic owner | unclear |
| 10 | Mapping `ChangeFeedError` to `sc_error` codes (`resync_required`, `invalid_cursor`) is delivery work, not 05 | AZ-3774 Outcome | E-12 read task or 11 | unclear |
| 11 | Items 3, 5, 6, 14 of batch 18 stay open (`reserve(0)` on an over-limit account, config defaults, nodes index, disk). Disk is now 66% used | batch 18 | See batch 18 | unclear |

## Commit

`[AZ-3751] [AZ-3727] [AZ-3774] quota lifecycle, group commit barrier, sync feed` — Loop: 1

## Next Batch

Recompute from `_docs/02_tasks/_dependencies_table.md` at the start of batch 20. Likely ready: AZ-3725 master keyring, AZ-3729 blob registry (one of the two, same crate), AZ-3839 media tables, AZ-3752 quota recompute (needs AZ-3751, now done), AZ-3696 sharing migration (needs the AZ-3695 spike), plus the spikes. 301 tasks left.
