# Batch 124, loop 1: AZ-3815 CLI acceptance and security on the compose stack (partly done, stays in todo)

**Batch**: 124
**Tasks**: AZ-3815_cli_acceptance_perf_security
**Date**: 2026-11-26

## What was built

- `tests/consumer/suites/operator_cli/test_operator_cli.py`:
  - AT-12-1 (AC-1): `doctor --pre-flight` is all Ok and prints the provider's redirect URI; the first admin signs in with the setup token; `doctor` exits 0.
  - ST-12-1 (AC-5): the tokens of `setup-token`, `create`, `invite create` and `reset` each appear once, on the stdout of the command that made them. None is on any stderr, in a SUT container log, in an audit row or in host `ps`, and the master key (hex, base64, raw) is nowhere, including the stdout of `doctor --json`, `user list`, `invite list` and `key-check`.
  - ST-12-2 (AC-6): `admin user delete`, `admin rewrap`, `admin epoch-bump` and `migrate revert` without `--yes` exit 3 with empty stdout and change no row, applied migration or epoch.
  - ST-12-3 (AC-7, runtime half): `doctor` and `scrub run --full` open no new listening socket in the api's network namespace.
  - ST-12-4 (AC-8): with the api's own connection string, `DROP TABLE` is refused with 42501 and `CREATE TABLE` is refused; the api holds no migrator secret or variable.
  - PT-12-1 and PT-12-2 (AC-3, AC-4) are `NOT-RUN(HW-REF)` markers.
- `test_restore_drill.py` (AT-12-2, AC-2): dump, then `epoch-bump`, then the dump loaded under a stopped api. `serve` refuses and exits, naming `admin epoch-bump`. In a one-off api container, `doctor` fails on the epoch, the bump gives a newer epoch, `scrub run --full` exits 0 and `doctor` exits 0 or 1. Then `serve` is ready, and a cursor from before the dump gets 410 `resync_required` / `epoch_changed`.
- ST-12-3 static half: gate 4 `cli-boundary` in `scripts/ci/check-db-gates.sh` (no `sqlx` and no `blobs/` path in `src/server/src/`, comments ignored). Its test seeds both violations and checks that each fails. The system-scope rule was already gate 2 (`run_system` allowlist).
- e2e-ctl (`ctl_actions.py`), with 6 new unit tests:
  - `exec-operator` gains `service: api-oneoff` (for when `serve` is down), `service: migrate` (`migrate ...` in the one-off migrate container) and `watch_listeners`.
  - `inspect` gains `logs`, `secret_scan` (counts only; it never returns a needle) and `db_role`. The database URL reaches psql on stdin, never on a command line.
  - `db-restore` gains `op: dump|load` with a name.
  - `db-probe` gains `schema_and_epochs`.
- `stackkit.wait_worker_ok`; the stack README is updated.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3815_cli_acceptance_perf_security | Partial: AC-1, AC-2, AC-5..8 pass; AC-3, AC-4 not written (reference server) | 11 files | stack 6/6 + 2 NOT-RUN; ctl 34/34; db-gates script test OK | AT-12-1 runs on the test image (release-image leg with AZ-3835) |

## Code Review Verdict: PASS_WITH_WARNINGS

Self-review of the diff. Warnings:
- The listener check compares ports only, not owning processes. A CLI that binds a port `serve` already uses would not show (it cannot: the bind would fail).
- The secret scan reads the logs of the SUT services, not the consumer's own pytest output.

## Test Suite

- Stack, `suites/operator_cli`: PASS=6 FAIL=0 NOT-RUN(HW-REF)=2 on a fresh stack.
- Full core profile on the same stack (`tests/run.sh rerun`): PASS=81 FAIL=0 NOT-RUN=6 (the four known media/sentinel rows plus PT-12-1, PT-12-2), 26 min 52 s.
- CI-parity: PASS (no Rust change; `scripts/ci/tests/test-check-db-gates.sh`, the ctl unit tests, `bash -n` and shellcheck on the changed scripts).
- `python3 -m unittest test_ctl_actions test_e2e_ctl`: 34 passed. `scripts/ci/tests/test-check-db-gates.sh`: OK.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `doctor` fails `media_worker` for up to 30 s after every worker start: the supervisor's first status report comes before its decoders pass the gate | AC-22 (doctor 0 after install) | Report on every slot state change; leftover `2026-11-26_worker-status-first-report.md`; the suites wait for a healthy report | clear (no guarantee changes) |
| 2 | A restored database keeps the `worker_status` row of its dump while `serve` is down, so `doctor` in the drill sees that row | AT-12-2 | The drill dumps after a healthy report, as a backup of a running install would | clear |
| 3 | The TTY leg of ST-12-2 ("with a TTY the confirmation names the target") is not on the stack; the binary tests of the guard cover it (IT-12-1) | AZ-3815 AC-6 | Left to the binary tests | clear |

## Next Batch

AZ-3835 AC-1 and AC-4 (integration suite runner, crate graph), then the media and worker suites (AZ-3847, AZ-3878).
