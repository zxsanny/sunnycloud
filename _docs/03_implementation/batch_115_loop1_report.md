# Batch Report

**Batch**: 115
**Tasks**: AZ-3835 AC-3 (all of IT-11-17 in Mode R) and the test-image leg of AC-2 (IT-11-9 in Mode R). AZ-3835 stays open: AC-1, AC-4, AC-5 and the release-image leg of AC-2 are not done here.
**Date**: 2026-11-24

## What changed

- Product: the real `/readyz` had no `migrations` item (only `database` and `scheduler_alive`), although AC-22 and AC-65 name it. `MigrationsReady` (`src/server/src/root/base.rs`) is registered in `ReadinessChecks` (`root/lists.rs`) and fails with `migrations pending`, or `migration state unknown` when the bookkeeping table cannot be read. Unit-level proof: `composition_scheduling.rs` reverts one migration on a real test database, sees the item fail, applies it, sees it recover.
- `tests/tools/e2e-ctl`: three real actions in `ctl_actions.py`. `internal-http` (one request from the host to the api on the backend network; `auth: worker` adds the worker bearer, which never reaches the consumer; header allow-list), `db-outage` (stop, or start and wait healthy) and `interrupted-upgrade` (the product's own `migrate revert --steps N --yes` and `migrate run`). A body is never written to the audit log, and a hung command or lost connection is an audited 500 `action_failed`. `run.sh` passes the project, compose file and secrets directory to ctl. 8 new unit tests.
- `tests/consumer`: `stackkit.py` (ctl client, edge request helper), `suites/http_api/test_it_11_9_listeners.py` (7 tests) and `test_it_11_17_readiness.py` (3 tests). The Dockerfile sets read permission on `/app` itself, because the host umask 007 left copied files unreadable for the consumer user (the same trap as in batch 114, now fixed at its source). The stack self-test that expected 501 from every action now expects 400 from the three implemented ones.
- Spec text fixed: IT-11-9 named `PUT .../{id}/status`; the real route is `PUT /api/v1/worker-jobs/status`, and the worker routes need an `X-Worker-Id` header (`components/11_http_api/tests.md`).

## Code Review Verdict: PASS (own review; no separate agent pass)

One finding fixed: an I/O error or timeout inside a ctl action would have dropped the connection with no audit line; it is now a typed 500 with an audit line (test added). Nothing else: the product change is one readiness item, the rest is test harness.

## Test Suite

- `tests/run.sh --profile core`: PASS=29 FAIL=0 NOT-RUN=1 (the sentinel), exit 0. IT-11-17 ran for real: the database stopped (`/readyz` 503 naming `database`, `/healthz` 200), restarted (200 again inside 60 s), one migration reverted (503 naming `migrations`), re-applied.
- CI-parity: PASS. `cargo fmt --all --check`; clippy `-D warnings` on `sunnycloud-server` with default, `vault` and `test-clock` features; `cargo nextest -p sunnycloud-server`: 307 passed, 5 failed, 2 skipped. The five are `pipeline_db_restore` and fail only because `pg_dump` 18 is not installed on this host (same five as before; CI installs it). ctl unit tests 21 OK.

## Not covered

- IT-11-9 release-image leg (`:9091` refused, clock-offset 404, `/test/clock` 404 on the public listener): the stack carries the test image only; the leftover `_process_leftovers/2026-11-18_az-3835-mode-r.md` says what the replay needs.
- The `key check` and `blob volume` readiness items that AC-22 names are still not registered in the real wiring.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| IT-11-9 text: `PUT .../{id}/status` | Real route has no job id; spec text corrected in this batch | clear |
| AC-22: readiness covers the key check and the blob volume | Only `database`, `migrations`, `scheduler_alive` are registered; add the other two with a task of their own (owners 04 and 02/12 register them) | clear |
| AZ-3835 AC-2: release image beside the test image | Needs a second api image build and service; replay in the leftover | clear |

## Commit

`[AZ-3835] Mode R of IT-11-9 and IT-11-17: migrations readiness item, real e2e-ctl actions, first suites on the compose stack` - Loop: 1

## Next: AZ-3654 first run (setup token through the stack; `serve` never calls `ensure_setup_token`), AZ-3664 sign-in, AZ-3662 share links, AZ-3665 account deletion
