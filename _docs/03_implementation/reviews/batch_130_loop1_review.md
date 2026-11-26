# Code Review Report

**Batch**: 130, AZ-4384_worker_suite_gate_decisions | **Date**: 2026-11-26 | **Verdict**: PASS_WITH_WARNINGS

Scope: the 09 change of commit 4f54286 (`decoder_client/conn.rs` `check_idle`, `slot_loop/mod.rs` `idle_checked`, the two
new worker test files, the rig's socket path, FD's `outage`), and this batch's harness and suite files
(`tests/tools/e2e-ctl/ctl_worker.py`, `ctl_actions.py`, `ctl_probes.py`, `actions.json`, the environment.md row,
`tests/consumer/{jobkit,fdkit,stackkit}.py`, `suites/media/conftest.py`, `suites/worker/*`, the Dockerfile `COPY`).

## Findings (final diff)

| # | Severity | Category | File:Line | Title |
|---|----------|----------|-----------|-------|
| F1 | Low | Maintainability | tests/consumer/suites/worker/test_it_09_03_05_decisions.py | `run_clip` swaps FD on both lanes per case (two supervisor restarts each); one session with a probe queue would be faster but would tie the cases together |
| F2 | Low | Maintainability | tests/consumer/suites/worker/test_it_09_03_05_decisions.py (IT-09-5) | `job_attempts` is the highest attempt count of every job of the module, not of the one transcode job; it holds today because no job of the module retries |

## Fixed during the batch (not open)

| Severity it would have had | Category | Where | What |
|---|---|---|---|
| Low | Maintainability | the four worker case files | `FAILED_ITEM`, `JOB_REQUESTS` and `job_requests` were repeated; now in `fdkit` (unit test added) |
| Medium | Bug | `slot_loop/mod.rs` `idle_checked` (before the commit) | a connection the last job had closed (an unhealthy decoder) was "checked" and logged as "ended while idle"; it now goes to the reconnect without that warning |

## Phases

- Spec compliance: AC-1 (IT-09-1 sweep, gate ok, gate failed, container contents; IT-09-15 RD and FD) and AC-3 (IT-09-3
  five decisions, IT-09-5, IT-09-8) each have a stack case; the gaps against 09 tests.md are recorded as rows 3 to 5 of the
  batch report (case times, backoff intervals, API access log).
- Security quick-scan: `scratch-fill` runs a fixed script with an integer argument, as an unprivileged uid, no network;
  `inspect container` returns variable names only (unit-tested against a secret value) and fixed media tool names; no new
  ctl action takes free-form input.
- Performance: the idle check is one non-blocking `recv(MSG_PEEK)` per slot round (every claim poll); nothing else on a
  hot path.
- Architecture: the worker gains rustix's `net` feature (rustix is already a dependency); no new crate, no new edge.
- Cross-task: `jobkit` replaces the media suite's private helper, so the media and worker suites share one way to commit
  and wait for jobs; FD swaps go through `fdkit` only.
