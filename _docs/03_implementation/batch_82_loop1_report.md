# Batch Report

**Batch**: 82
**Tasks**: AZ-3796_epoch_restore_gc_commands (AC-6 only; the rest was batch 77), now complete
**Date**: 2026-11-17

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3796 AC-6 `admin restore --dry-run` | Done | `contracts/src/tree/maintenance.rs`, 02 `restore.rs`, `server/src/maintenance_commands.rs`, `contracts/tests/tree.rs`, 02 `tests/tree/{restore_preview.rs (new), restore_support.rs, main.rs}`, `server/tests/maintenance_commands.rs` | See Test Suite | None open |

Owner decision of 2026-11-17 (asked in this session): a new `TreeRestore::preview_restore(account, target, at)`, additive, same arguments and result as `restore_revision`. `PgTreeRestore` runs the real restore transaction (`restore_in`: lock, path resolution, revision choice, `reinstate_in_tx`, placement, usage, feed stamp) and ends the closure with an error that carries the outcome, so the transaction always rolls back. The retention pre-check, `NotFound` and `FileMissing` are therefore the real ones, and the notifier is never called. `admin restore --dry-run` calls it and prints `revision`, `placement` and `changed: false`; a real restore prints `changed: true`.

Tests: 02 against a real database (`restore_preview.rs`): the preview equals the later real restore and leaves rows, usage, feed `seq` and blob state untouched; a preview under a deleted folder says `Restored` and creates no `Restored` folder; `BeyondRetention`, `NotFound` and `FileMissing` come back from a preview with every row unchanged. CLI: `--dry-run` makes one `preview_restore` call and no `restore_revision` call; a preview refusal exits 2.

Docs: contracts part 2 (new row), `tree_service.md` (1.3.0), AZ-3796 spec (AC-6 and the resolved concern).

## Code Review Verdict: PASS

No findings. Cost of the chosen design: a preview takes the same account advisory lock and does the same writes as a restore before rolling back, so it is as heavy as a restore (operator-only, one account).

## Test Suite

- `contracts`, `sunnycloud-tree`, `sunnycloud-server` (the crates whose code changed), `--no-fail-fast`: 497 passed, 1 failed. The failure, `publish_concurrency::fifty_concurrent_commits_feed_each_seq_once_in_order_one_hundred_times`, panicked at `publish_concurrency.rs:40` (`publish` returned an error) while the host load average was about 20 (other users' jobs share this machine). It failed once more alone at that load, then passed alone (117 s) at load 6. The same test flaked under load in batch 76. The 02 changes touch only `restore.rs`; the test does not call a restore. Recorded as load-sensitive, unrelated.
- The three new 02 preview tests, the existing restore tests (15 in total), and the 2 new CLI tests pass.
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.
- Workspace: not re-run this batch (one full-suite gate at the end of the implementation phase).

## Commit

`[AZ-3796] Restore dry run through TreeRestore::preview_restore` - Loop: 1

## Next Batch: remaining owner questions (AZ-3795 AC-3/AC-4, AZ-3793 AC-6, AZ-3809, AZ-4022) or the unread small tasks (AZ-3824, AZ-3835, AZ-3926, AZ-3971)
