# Code Review Report

**Batch**: 133, AZ-4387_worker_suite_input_fetch | **Date**: 2026-11-27 | **Verdict**: PASS_WITH_WARNINGS

Scope: `src/worker/fake-decoder/src/input.rs` (new), `record.rs`, `lib.rs`, `Cargo.toml`; e2e-ctl `ctl_worker.py`
(`bit-flip`, `job-row-edit`, `worker-variant`), `ctl_actions.py` (handlers, `_compose(extra_files)`, `inspect logs
lines_with`), `ctl_probes.py` (`job_rows`), `actions.json`, the unit tests; `tests/h2-variants/*.yml`; the suite
`h2cases.py`, `test_it_09_07_input_fetch.py`, `test_it_09_06_i_fetch_5xx.py`; environment.md and 09 tests.md rows.

## Findings (final diff)

| # | Severity | Category | File:Line | Title |
|---|----------|----------|-----------|-------|
| F1 | Low | Performance | src/worker/fake-decoder/src/input.rs:23 | FD reads and hashes the whole `in/source` on every job request of a job (probe, then each render or transcode) |
| F2 | Low | Maintainability | tests/tools/e2e-ctl/ctl_worker.py:54 | the variant names are listed in `WORKER_VARIANTS` and again as the files of `tests/h2-variants/` |

Fixed after the cumulative review's evidence pass (findings 1, 4, 6, 8 there): `input_errors` counted lines in a 4 KiB
log tail, so after a full run the (i) case would have waited out its timeout (Medium, Bug; now a time mark); the
`worker_variant` and `lanes` fixtures recorded their change only after the call succeeded (Low); `worker_state` could
raise inside a failure message (Low); the README's action list and row count (Low).

Fixed during the review: two triple blank lines (`h2cases.py`, `ctl_worker.py`; Style); `job_rows` split a row on every
colon, so a failure code holding one would have broken the parse; it now takes the code as the text between the fourth
and the last colon, after the job kind the cumulative review added (Maintainability).

### Finding Details

**F1: FD hashes the input on every job request** (Low / Performance)
- Location: `input.rs` `facts`, called from `Recorder::request` for each job request when a record file is given.
- Description: a clip's poster and transcode jobs each read their input again per request. The suite's inputs are at
  most 2 MiB (tens of ms); a multi-GiB input would cost seconds per request, but FD never answers a real-media case
  (RD does).
- Suggestion: none now; hash once per job directory if FD ever gets large inputs.

**F2: variant names in two places** (Low / Maintainability)
- Location: `WORKER_VARIANTS` and `tests/h2-variants/`.
- Description: a file added without its name is unreachable; a name without its file fails the unit test
  `test_every_listed_variant_but_default_has_its_override_file`. Only the second direction is checked.
- Suggestion: none; the fixed list is the point (no file name from a request reaches a command).

## Phases

- Spec compliance: AC-1 (a), (b), (d), (e) each have a case on the real API; (c) is NOT-RUN with the D2b waiver and
  the pointer to `src/worker/tests/api_input.rs:175`. (b)'s "scratch `source` size 0" cannot be read after the job
  ended (the tree is removed with the failure); `api_input.rs:108` covers it, the stack case asserts `TooLarge`, dead
  after one attempt, no decoder request and an empty scratch. AC-2: two or more 5xx answers of the real input route
  (the api's own log lines), `Transient`, the job queued again, the second answer at least the 1 s backoff later.
- Security quick-scan: the id parameters of `bit-flip` and `job-row-edit` are parsed as UUIDs before they reach a shell
  argument or the one fixed SQL statement; `worker-variant` takes a name from a fixed set and builds the file path
  itself; `lines_with` is a substring test on log lines, never a pattern or a command argument. The bit-flip helper
  runs as root in a throwaway container with no network and only the test stack's blob volume.
- Performance: test tooling only (F1).
- Architecture and cross-task: FD's new edge to `sc_ids` (shared) is one the worker already has; the cases go through
  `fdkit`, `jobkit`, `h2cases` and ctl only; `run_file` now starts from `fd_on_general`, so the gate-out of the
  interactive slot exists once.
