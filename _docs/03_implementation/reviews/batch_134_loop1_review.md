# Code Review Report

**Batch**: 134, AZ-3878 AC-7 | **Date**: 2026-11-27 | **Verdict**: PASS_WITH_WARNINGS

Scope: e2e-ctl `process-kill` `stop`, probe `worker_status_report`, the unit test; `h2cases.py` (moved and new helpers),
`test_it_09_10_lease.py` (helpers moved out), `test_it_09_17_shutdown.py`, `test_it_09_18_test_clock.py`; 09 tests.md
IT-09-18, environment.md Process kill row, the D3 record.

## Findings

| # | Severity | Category | File:Line | Title |
|---|----------|----------|-----------|-------|
| F1 | Low | Maintainability | tests/consumer/suites/worker/test_it_09_17_shutdown.py | the "running photo at the stop" rests on a margin (2 s job against the commit and the stop call) rather than a hold |
| F2 | Low | Maintainability | tests/tools/e2e-ctl/ctl_worker.py `stop_timed` | the reported seconds include `compose` start-up (a few hundred ms), so the case's lower bound is the grace minus 0.5 s |

### Finding Details

**F1** (Low / Maintainability): a hold on the render (FD answering only on a signal) would remove the margin; FD has no
such answer yet, and the margin is documented beside `RENDER_MS`. **F2** (Low / Maintainability): documented in the case
bound; a stop that ignored the grace would take under 1 s or 10 s (SIGKILL, exit 137), both outside the bound.

## Phases

- Spec compliance: IT-09-17 asserts every listed result (no new claims, the running job posts, `cancel` at the grace,
  lease released with attempts back, exit 0 within 5 s, status every second with `decoder_ok`, `image_age_days` 20 and
  the API's clock). IT-09-18 follows the reworded text (D3); the two items not visible on the stack point at the crate
  tests.
- Security quick-scan: `stop` is allowed for the worker only; no new parameter reaches a shell or SQL.
- Performance: test code only.
- Architecture and cross-task: the heartbeat helpers exist once (`h2cases`), used by IT-09-10 and IT-09-18.
