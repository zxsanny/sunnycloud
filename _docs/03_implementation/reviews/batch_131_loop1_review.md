# Code Review Report

**Batch**: 131, AZ-4385_worker_suite_faults_slots | **Date**: 2026-11-26 | **Verdict**: PASS_WITH_WARNINGS

Scope: `tests/consumer/suites/worker/h2cases.py`, `test_it_09_06_retry.py`, `test_it_09_11_watchdog.py`,
`test_it_09_12_hostile_outputs.py`, `test_it_09_13_hostile_frames.py`, the move of `test_it_09_03_05_decisions.py` to
`h2cases`; the split specs, the dependency rows, the D2 card.

## Findings (final diff)

| # | Severity | Category | File:Line | Title |
|---|----------|----------|-----------|-------|
| F1 | Low | Maintainability | tests/consumer/suites/worker/test_it_09_13_hostile_frames.py | `probe_reply` repeats FD's `hostile.rs` builder in Python; the two can drift (the suite cannot call the Rust one) |
| F2 | Low | Maintainability | tests/consumer/suites/worker/test_it_09_12_hostile_outputs.py | the FIFO bound is "the job left `running` within 3 s" of the render request, wider than the doc's 1 s, because the suite polls the job state through ctl |

## Phases

- Spec compliance: AC-4 legs each have a case; the two NOT-RUN legs carry their reason and a pointer to the unit test
  that covers them; the scaled bound of IT-09-11 and the PUT-body gap of IT-09-12 are rows 3 and 4 of the report.
- Security quick-scan: FD's symlink case points at `/run/secrets/worker_api_token`, which exists only in the worker's
  container; FD runs without network and as uid 10003; nothing new in ctl.
- Performance: test code only.
- Architecture and cross-task: the cases go through `fdkit`, `jobkit` and `h2cases` only; no second way to swap a decoder
  or to wait for jobs.
