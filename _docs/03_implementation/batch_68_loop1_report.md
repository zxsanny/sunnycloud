# Batch Report

**Batch**: 68
**Tasks**: AZ-3703_sharing_security_perf_suites (done; PT timing `NOT-RUN(HW-REF)`), AZ-3760_transfer_integration_pass (done)
**Date**: 2026-11-14

Two parallel workers: 06 (AZ-3703, owner option A) and 03 (AZ-3760). Both are test tasks; no production source changed and no defect was found. The parent allowlisted 03's test-only account helper `tests/transfer/real_account_repo.rs` (same shape as 02's from AZ-3720: 01's account insert plus the real `initialize` hooks in one system-scope transaction), reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3703_sharing_security_perf_suites | Done | New `share_security` binary: ST-06-1 (account B cannot create, list or revoke on A's link), ST-06-2 (10,000 tokens: `sc_shr_` + 32 bytes, distinct, chi-square uniform at p = 0.001, only SHA-256 stored, never in rows, audit or logs — with both abuse WARN lines captured so the log check is not vacuous), ST-06-3 (10,000 guesses: one answer, address limit trips at call 101), ST-06-7 (four file fields, no identifier), ST-06-8 (B reads only its rows; exact-token lookup). New `share_perf` binary: PT-06-1..3 functional halves pass; timing halves `#[ignore = "NOT-RUN(HW-REF)"]`. ST-06-4/5/6 already exist (AZ-3699, AZ-3701) and run in the same suite |
| AZ-3760_transfer_integration_pass | Done | Real 01 `IdentityAccounts` as the guard (02 and 03) and 08's `MediaEnqueueEffect` in 03's effect slot: AC-1 every transfer entry point refused for a `deleting` account with nothing written (seq unchanged); AC-2/AC-3 jobs only for committed images, none for an aborted session, none on a replay; AC-4 effect order guard < lock < session/receipt < job insert < receipt insert < stamp, two identical batches apply once, a failed entry rolls back alone with no job, 100 reaper-vs-commit rounds with one winner each and no double release. The Medium flag closed by the 2026-11-13 owner decision (03 takes `Arc<dyn AccountGuard>`) |

## Code Review Verdict: PASS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18: 3661 passed, 0 failed, 31 ignored, exit 101 only because the parent killed the hung 04 `reader` binary. Cause: the batch 67 fix of `window::a_consumer_slower_than_the_disk_stays_at_one_chunk_ahead` captured the read count after the next read could already have finished, so its wait loop could wait for a read that never came. Fixed by waiting for `taken + 1` completed reads in all (the open reads the first chunk); same assertions; the window tests 5 of 5 and the whole `reader` binary 20 of 20 green, 04 clippy clean. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no migration in this batch.

Notes (recorded):
- PT-06-3's CI half cannot show the per-link limit refusing (the 4 permits slow attempts below the window); only "Argon2 reached" is asserted. The attacker model after a refusal (waits `retry_after`) is the worker's reading — E-16 owner to confirm.
- `tests.md` PT-06-2 still mentions `share_lookup_cache`, removed by the 2026-11-14 owner decision (noted in `tests.md`).
- `PgShareService` still has no `impl PublicShareService` (`authorize_derived` missing); 11 uses a double.
- 03's `start_support.rs` is at 495 lines.

## Parent changes

- `scripts/ci/run-system-allowlist.txt`: `src/components/03_transfer/tests/transfer/real_account_repo.rs`.
- 04 `tests/reader/window.rs`: race-free wait in the slow-consumer test (see CI-parity).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | 03 test account helper needs system scope | AZ-3760 | Allowlisted (`real_account_repo.rs`) | clear |
| 2 | 03's own `initialize` (writes nothing) is not run in the real rig | AZ-3760 | Documented | clear |
| 3 | "08 ran, then a later effect fails" not reachable with real 03 effects only | AZ-3760 | Covered by 02's AZ-3720 injected-fault test | clear |
| 4 | PT-06-2 needs the invalid-token limit raised for its single client | AZ-3703 | Its own limiter; every `NotFound` still records | clear |

## Commit

`[AZ-3703] [AZ-3760] Sharing security suites, transfer integration pass` — Loop: 1

## Next Batch

Recompute the ready tasks.
