# Batch Report

**Batch**: 116
**Tasks**: AZ-3654 first-run bootstrap pipeline (AC-1 to AC-5). Jira: In Testing (the first two attempts timed out in a hook; the replay on 2026-11-24 succeeded)
**Date**: 2026-11-24

## What changed

- Product: `serve` never issued the setup token, so a fresh install could not get a first admin from its own start (the gap the spec names). `serve` now ends its start with a step "serve start: setup token" (`src/server/src/serve/setup.rs`): with zero accounts it calls `ensure_setup_token` and prints the token once on stdout through the same one-time-secret writer as `admin setup-token` (`token: ...`). A restart replaces the token. With `--json` nothing is printed and nothing is issued (a token must not appear in JSON output); the log says to run `admin setup-token`. A token never reaches a log line. To pass the output mode to `serve`, `Call` (`handler.rs`) carries it.
- Binary tests on a real PostgreSQL and the real `sunnycloud serve` (`tests/serve_setup_token.rs`, 4 tests): one token on stdout and only its SHA-256 stored; a restart replaces it; `--json` prints and issues none; an install with an account prints none. `tests/wiring_first_run.rs`: the seven operations of flow F1 each have a non-test caller (AC-5). `serve_support` lists the new start step.
- Stack suite `tests/consumer/suites/first_run` (6 tests) through Caddy, the real routes and the mock identity provider: the first sign-in with the token makes the first admin and the session works; a used token is refused; a sign-in without the token creates no account (`invitation_required`); two concurrent sign-ins with one token make exactly one admin; a new setup token replaces the old; an admin invite makes a second admin and is single use.
- Harness: ctl actions `exec-operator` (`sunnycloud admin|doctor|key-check` in the api container) and `db-restore` (the golden snapshot through `run.sh reset`), a web client in `stackkit.py`. ctl turns an unexpected error into an audited 500 and never kills its handler thread; `internal-http` refuses a container without an address.
- Found on the way, and fixed in the harness: the golden snapshot held only the database, while the first api start writes a `.epoch` marker into the blob volume; after a restore the api correctly refused to start ("database epoch 0 is below the .epoch marker"). The snapshot now covers the blob volume as well, and `reset` stops api, worker and database before restoring both. `tests/tools/check-reset.sh` passes (16 s, budget 60 s).

## Code Review Verdict: PASS (own review)

Findings fixed during the batch: an IP check in ctl that could kill a handler thread; a session check that relied on a cookie name (now an authenticated request). The production change is one step in `serve` and one field in `Call`.

## Test Suite

- `tests/run.sh --profile core`: PASS=35 FAIL=0 NOT-RUN=1 (the sentinel), exit 0.
- CI-parity: PASS. `cargo fmt --all --check`; clippy `-D warnings` on `sunnycloud-server` with default, `vault` and `test-clock`; `cargo nextest -p sunnycloud-server`: 312 passed, 5 failed, 2 skipped. The five are `pipeline_db_restore` and fail only because `pg_dump` 18 is not installed on this host (CI installs it). ctl unit tests 24 OK.
- AC-3 (start refusals) is covered by existing binary tests: `serve_start.rs` (pending migration names `migrate`) and `config_binary.rs` (no provider names `providers`).

## Not covered

- NFR "start to `/readyz` 200 within 10 s" is asserted by `serve_start.rs` (existing); the 30-minute install measurement is AT-12-1 (E-22/c17).

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| AC-1 says the token is read "from the log"; the code prints it on stdout (the secret channel) | Stdout, as the spec's Risk 1 says (secret channel only); spec wording is loose | clear |
| `--json serve` prints and issues no setup token | Follows the spec's NFR (never in JSON output); the operator runs `admin setup-token` | clear |
| A restored database needs `admin epoch-bump` once the api has run | Product behaviour (AC-65), unchanged; the harness restores both volumes instead | clear |

## Commit

`[AZ-3654] serve issues and prints the setup token; first-run suite on the compose stack; golden snapshot covers the blob volume` - Loop: 1

## Next: AZ-3664 sign-in pipeline, AZ-3662 share links, AZ-3665 account deletion on the stack
