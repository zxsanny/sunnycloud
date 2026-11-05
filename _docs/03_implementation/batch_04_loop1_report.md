# Batch Report

**Batch**: 4
**Tasks**: AZ-3765_sc_clock, AZ-3889_ipc_schema_crate
**Date**: 2026-11-05

Two parallel workers on disjoint paths (`src/shared/clock/` plus two CI scripts and a small `ci.yml` edit; `src/desktop/ipc_schema/`). The other ready tasks are spikes that need owner decisions.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3765_sc_clock | Done | clock crate (7 src, 6 test files), 2 CI scripts, 3 script tests, ci.yml (2 steps) | 27 tests, with and without `test-clock`; no-alloc proof; SQL `now()` gate and symbol check tested | Release-symbol check passes vacuously until a binary links sc_clock |
| AZ-3889_ipc_schema_crate | Done | ipc_schema crate (9 src files, 6 test files) | 53 tests | 10 open questions (see below) |

## Code Review Verdict: PASS_WITH_WARNINGS

Focused review of the joined tree: 5 findings (unbounded IPC line buffer, vacuous symbol check on the skeleton, negative advance not expressible, boxed sleep future, required status lists); 1 no change needed, 4 skipped with reasons.

CI-parity: PASS. In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, wasm32 build of `sync-core-wasm` and `sc_ids`, `cargo deny check advisories bans licenses sources`; from the root `scripts/ci/check-pinning.sh` and `scripts/ci/check-no-sql-now.sh`. The new `images` step (release symbols from the built images) runs for the first time on GitHub.

## Test Suite

- Total: 166 (workspace, default features)
- Passed: 166
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The contract wants `NegativeAdvance` for `-1s`, but `Duration` is unsigned; `advance_seconds(i64)` added beside `advance(Duration)` | AZ-3765 AC-3 | Contract says which E-21 calls | unclear |
| 2 | Wake-gap compares wall elapsed minus monotonic elapsed to the gap, forward jumps only; exactly the gap is false | AZ-3765 AC-8 | E-24 confirms | unclear |
| 3 | `sleep_until_monotonic` returns a boxed future; shutdown is a `CancellationToken` (`new`, `with_shutdown`, `shutdown`) | AZ-3765 | Contract text update | clear |
| 4 | SQL gate checks `now()` only, not `CURRENT_TIMESTAMP`, `clock_timestamp()` and similar | AZ-3765 AC-5 | Add patterns if wanted | unclear |
| 5 | `test-clock` is not forwarded to server and worker yet; forward it through each crate when the first consumer links sc_clock | AZ-3765 AC-4 | Consumer tasks | clear |
| 6 | `scripts/ci/README.md` does not list the new scripts | AZ-3765 | Add in Step 13 | clear |
| 7 | Notification method name `status_event`, -32600 mapping, `data` shapes for -32700/-32600/-32602/-32601, and the `StatusSink` carrier were chosen by the worker | AZ-3889 | Contract update | unclear |
| 8 | No maximum line length in `LineBuffer` (contract open point 8) | AZ-3889 | Decide a cap before schema freeze | unclear |
| 9 | `bandwidth_limit` unit (G-AS-2), `expires_at` as `String`, per-file errors and endpoint names still open | AZ-3889 | Owner | unclear |
| 10 | `access_denied` and `watcher_degraded` lists are required in `DaemonStatus` | AZ-3889 AC-6 | Default them for older daemons | unclear |

## Commit

`[AZ-3765] [AZ-3889] sc_clock and ipc_schema crates` — Loop: 1

## Next Batch: no non-spike task is ready except those that depend on AZ-3765 (now done): AZ-3766 sc_ids, AZ-3767 sc_names, AZ-3785 sc_ratelimit
