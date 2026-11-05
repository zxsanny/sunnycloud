# Batch Report

**Batch**: 3
**Tasks**: AZ-3761_ci_skeleton, AZ-3742_test_harness_skeleton
**Date**: 2026-11-05

Two parallel workers on disjoint paths (`.github/` and `scripts/ci/` for 3761; `tests/` for 3742). Neither touched `src/`.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3761_ci_skeleton | Done locally; first GitHub run pending | `.github/` (2 files), 14 scripts, 10 script tests | Fixture and real-tool tests pass; coverage 94.05% on the skeleton; real 3-minute watchdog test passed | AC-1 and in-job behaviour unproven until a GitHub run |
| AZ-3742_test_harness_skeleton | Done | `tests/` (run.sh, compose.test.yml, consumer, e2e-ctl stub, README) | `core` profile: 15 PASS, 0 FAIL, 1 NOT-RUN, exit 0, about 30 s; e2e-ctl 10 unit tests | None |

## Code Review Verdict: PASS_WITH_WARNINGS

Focused review of the joined tree (workflow, pinning, secrets handling, harness): 5 findings, all skipped with reasons (hash-pinned pip, invented e2e-ctl names and NOT-RUN code, real watchdog test not in CI, gitleaks allowlist, repeated workflow blocks).

CI-parity: PASS. In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`; from the root: `scripts/ci/check-pinning.sh`, `scripts/ci/tests/test-check-pinning.sh`. The secret scan and `run-script-tests.sh` ran in the worker's environment (gitleaks is not installed on this workstation).

## Test Suite

- Total: 98 Rust tests (unchanged), plus the script and harness suites above
- Passed: 98
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Coverage gate at 75% overall passes at 94.05%; crates without instrumented lines are not counted | AZ-3761 AC-6 | Keep thresholds; decide later whether `main.rs` stubs should count | clear |
| 2 | `scripts/ci/gitleaks.toml` allowlists task-id strings (3 false positives in `_docs/02_tasks`) | AZ-3761 AC-8 | Owner approves the allowlist | unclear |
| 3 | `audit.sh` runs `cargo deny check` itself, so the workflow has no separate deny step | AZ-3761 | Keep | clear |
| 4 | Branch protection (required checks) and `COPYING.iOS` remain owner actions | AZ-3761 flagged concerns | Owner | unclear |
| 5 | NOT-RUN code `NO-SERVICE` chosen for "service not available in this build yet" | AZ-3742 flagged concern | Test owner confirms; add to `environment.md` | unclear |
| 6 | 18 of 22 e2e-ctl action names are kebab-case derivations of the table rows | AZ-3742 AC-5 | Test owner confirms | unclear |
| 7 | `environment.md` still names `worker-supervisor`, `proxy-net`, `backend-net`; the harness uses `worker`, `proxy`, `backend` | AZ-3742 | Doc fix in Step 13 | clear |
| 8 | `reset` needs `--keep` (secrets and stack); a plain run deletes both | AZ-3742 AC-6 | Keep | clear |
| 9 | Python dependencies in the consumer are pinned but not hash-checked | AZ-3742 | Add hashes when the consumer grows | clear |

## Commit

`[AZ-3761] [AZ-3742] CI skeleton and test-harness skeleton` — Loop: 1

## Next Batch: AZ-3765 sc_clock, AZ-3767 sc_names (needs 3765), AZ-3766 sc_ids (needs 3765, 3761), AZ-3785 sc_ratelimit (needs 3765); then AZ-3782 sc_db_access
