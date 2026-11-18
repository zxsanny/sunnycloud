# Batch Report

**Batch**: 87
**Tasks**: AZ-3656_pipeline_small_upload_batch, AZ-3655_pipeline_large_upload, AZ-3835_integration_suite_crate_graph (parallel wave: one worker for both pipelines in sequence, one for AZ-3835; parent review and fixes)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3656 | Done | `server/tests/pipeline_small_batch.rs`, `pipeline_support/{mod,h2,wiring}.rs`, `serve_support/account.rs` (`Stack::device_token`), `vault_support/mod.rs` (uses it) | 6/6, 3 runs | None open |
| AZ-3655 | Done (owner chose one task with AC-68, 2026-11-18) | `server/tests/pipeline_large_upload.rs`, `pipeline_support/large.rs`, `serve_support/mod.rs` (`Server::crash`, SIGKILL) | 9/9, 3 runs | Discovered #1, #2 |
| AZ-3835 | Partly, stays in todo (AC-2, AC-3 NOT-RUN(NO-SERVICE)) | `scripts/ci/{it_suite.py,check-it-suite.sh,coverage_gate.py,check-coverage.sh,critical-paths.txt,README.md}`, `scripts/ci/tests/{test-it-suite.sh,test-check-coverage.sh,run-script-tests.sh}`, `.github/workflows/ci.yml` (job `http-api-suite`), `src/components/11_http_api/tests/it_cases.txt`, `server/tests/crate_graph.rs` | runner 20/20 cases; crate_graph 19; script tests 6/6 and 10/10 | Coverage not measured locally (no `cargo-llvm-cov`); Mode R waits for the SUT in the test stack |

No production code changed. Server dev-dependencies added by the parent for AZ-3656 AC-1 (HTTP/2): `hyper` (client, http2), `hyper-util` (tokio), `http-body-util`, the versions 11's tests use; the lockfile gains edges only.

Durations: AZ-3656 AC-3/AC-5 and AZ-3655 AC-5/AC-68 run on short config values (`upload_session_ttl` 1m, `orphan_sweep_age` 1m, janitor intervals 1-2 s) and poll, never on a test clock: `tests/environment.md` keeps the test clock for ACs that assert a production default (parent decision 2026-11-18). The serve binary has no test clock anyway (Discovered #5).

AZ-3835: `it_suite.py` reads the case ids and max execution times from `components/11_http_api/tests.md` and the id-to-test map from `it_cases.txt` (id, target, test-name prefixes); it fails on a case with no line, a prefix that selects nothing, an unknown id, a failing run or a case over budget. Crate graph: 11 may not take `sc_db` (or a component, or `sqlx`) as a normal dependency, nor a component take 11; planted edges are proven on the real graph with synthetic additions. Coverage: 90% entries for 11's `csrf`, `auth`, `problem.rs`, `port_error`, `content`; crate floor `components/11_http_api/src 75` (new per-entry minimum in `coverage_gate.py`); the first real measurement is the next `rust-test` CI run. Leftover: `_docs/_process_leftovers/2026-11-18_az-3835-mode-r.md`.

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Spec-Gap | `pipeline_support/mod.rs` | AZ-3656 AC-1 staged over HTTP/1.1 connections: the server tests had no HTTP/2 client | **Fixed**: h2c dev-dependencies added; 16 parallel streams on one connection; the helper asserts the response version |
| 2 | Medium | Maintainability | `vault_support` / `serve_support` | Two copies of the device-token seed | **Fixed**: one `Stack::device_token` |
| 3 | Low | Test | `pipeline_large_upload.rs` AC-68 in-flight kill | The test accepts either outcome (whole revision, or cleaned-up upload); only the cleaned-up branch was observed | Kept: the kill point cannot be chosen from outside the process; both branches assert the cross-component invariant |
| 4 | Low | Test | AC-4 deleting-account commit | Reaches the account guard only because the login is cached (`session_cache_ttl` ≤ 3 s); a very slow host could see 401 at login instead | Kept, noted (Discovered #1) |
| 5 | Low | Docs | `scripts/ci/README.md` | No row for the new runner and job | **Fixed** (parent) |

## Test Suite

Joint run: `pipeline_small_batch` 6, `pipeline_large_upload` 9, `crate_graph` 19, `composition_root` 12, `serve_stop` 3, `vault_http`/`vault_report` default and `--features vault` (4 + 1), `sunnycloud-http-api` 86 + 361 (1 ignored, pre-existing) + 1, `check-it-suite.sh` 20/20 within budget, `test-it-suite.sh` and `test-check-coverage.sh` OK. 0 failures.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning` exit 0; `run-script-tests.sh` as a whole NOT-RUN(env: no `gitleaks`), its other script tests pass one by one. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | A commit from a `deleting` account reaches the account guard only while the device login is cached (≤ 3 s); past that, login answers 401 before any commit | AZ-3655 AC-4 | Accept, or add a deterministic way to reach the guard | unclear |
| 2 | AC-68 says the orphan sweep removes the crashed upload's file; in fact the session reaper deletes the session and blob rows and the file, so the orphan sweep never sees an orphan (a file with no row cannot be produced through the routes) | AZ-3655 AC-68 | Reword to "the file is removed (reaper; orphan sweep as backstop)", or add an orphan-sweep scenario | unclear |
| 3 | A committed large session keeps its `upload_sessions` row as a committed record until `upload_session_ttl` | AZ-3655 AC-4, F4 step 5 | None: matches the contract | clear |
| 4 | AC-6 names `BlobWriter::create` / `write_complete`; the contract method is `create_file`, and `write_complete` is only on the staged path | AZ-3655 AC-6 | Align the spec names | clear |
| 5 | The server feature `test-clock` is empty: the `-test` images (`build-images.sh`, `FEATURES=test-clock`) get neither 11's `:9091` listener and clock-offset route nor an `OffsetClock`; `wiring.rs` always builds `SystemClock` | AZ-3835 AC-2, `http_server.md` Runtime handles | Recorded in the Mode R leftover; fix with E-31 or as its own task | clear |
| 6 | Coverage of 11's five critical areas not measured locally (`cargo-llvm-cov` not installed) | AZ-3835 AC-5 | Watch the first `rust-test` run; real tests if an area is under 90% | unclear |

## Commit

`[AZ-3656] [AZ-3655] [AZ-3835] Upload pipelines and the IT-11 suite runner` — Loop: 1

## Next Batch: recomputed after this commit (AZ-3664 waits for mock-idp, E-31/c13; AZ-3971 for the SUT in the test stack)
