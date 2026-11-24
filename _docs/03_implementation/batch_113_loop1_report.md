# Batch Report

**Batch**: 113
**Tasks**: AZ-3795_user_seed_audit_commands (AC-4: `admin dev-seed`; AC-1, 2, 3, 5 were done in batches 77, 78 and 83)
**Date**: 2026-11-24

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3795 AC-4 `admin dev-seed` | Done | `server/src/{dev_seed_commands,dev_seed_login,dispatch,lib}.rs`, `server/src/root/{seed,mod}.rs`, `server/tests/{dev_seed_commands,binary,exit_codes}.rs` | See Test Suite | none |

`admin dev-seed` makes one `DevSeed::seed` call. The owner's decision of 2026-11-17 is applied: the development `ProviderLogin` is an HTTP login at the mock identity provider (`server/src/dev_seed_login.rs`). It opens the authorization URL with the seed subject as `login_hint`, does not follow the redirect, and returns the `code` and `state` of the `Location`. Plain `http` only; anything else is refused before a connection. The composition root registers `dyn DevSeed` (`root/seed.rs`) from the builders every other consumer uses; the provider is the first configured `oidc` provider, the host is the authority of `public_url`. Production is refused by 07 (exit 2, code `forbidden`); any other failure exits 2 with the reason.

The `dispatch` match no longer has a catch-all `NotImplemented` arm: every command has a handler, and a command added to the tree without one does not compile. The two tests that used `admin dev-seed` as their "unfinished command" are removed.

Two defects found only by a real run (the unit tests with recording ports cannot show them), both fixed in `root/seed.rs`:
- sign-in refused to create an account (`account_hooks_not_registered`): `serve` registers 01's account hooks before it accepts a request; the seed builder now builds `AccountHooks` too.
- the demo upload failed with `master key unavailable`: `serve` calls `load_and_verify` on the key ring at start; the seed builder now does the same.

## Code Review Verdict: PASS_WITH_WARNINGS

Four findings, none blocking. Fixed: decoder edge-case test added. Left: a seed that fails after creating the accounts is a no-op on rerun (07's rule, pre-existing; drop the throwaway database and rerun); the output says `seeded: true` also when nothing was created; the production refusal comes after the ring load and the database connection.

## Test Suite

- Real run (not CI): a fresh database in the acceptance DB container, the debug `sunnycloud` binary, a mock IdP on its own port with the three seed users. `migrate`, `keycheck-init`, `admin dev-seed` created the admin and two users with demo content (users `used: 27` bytes, one limit set); a second run changed nothing; `SUNNYCLOUD_ENVIRONMENT=production` printed the refusal. The throwaway database was dropped.
- `sunnycloud-server` under nextest `--no-fail-fast`: 310 run, 305 passed, 5 failed: all five are `pipeline_db_restore` (`pg_dump` of PostgreSQL 18 is not on this host; CI installs it). Not related to this batch.
- New: 3 command tests (`dev_seed_commands.rs`), 6 unit tests (`dev_seed_login` 5, `root/seed` 1).
- CI-parity: `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql` exit 0. `CI-parity: PASS`.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| IT-12-17 on the E-31 stack | The real run above needs a mock IdP with the three seed users (`dev-seed-admin`, `dev-seed-user-1`, `dev-seed-user-2`); the E-31 compose stack must configure them | clear |
| 07 `DevSeed` | A half-failed seed is not resumable | unclear (pre-existing, Low) |

## Commit

`[AZ-3795] admin dev-seed: HTTP login at the mock identity provider; every command now has a handler` - Loop: 1

## Next: roadmap item 4, the E-31 compose test stack (api, Caddy, mock IdP with the seed users) and its decompose
