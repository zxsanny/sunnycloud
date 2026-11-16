# Batch Report

**Batch**: 78
**Tasks**: AZ-3795_user_seed_audit_commands (partly, stays in `todo/`)
**Date**: 2026-11-16

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3795_user_seed_audit_commands | **Partly, stays in todo** (AC-1, AC-2, AC-5 done; AC-3 and AC-4 blocked) | `user_commands.rs`, `audit_commands.rs` (new), `dispatch.rs`, `lib.rs`, `root/domain.rs`, `root/base.rs`, `tests/user_commands.rs`, `tests/audit_export.rs`, `tests/binary.rs` | 6/6 + whole server crate 184/184 | See Blocked |

`admin user list`: pages of 100 through `dyn AdminService::list_users(&Actor::Operator, ..)` until an empty page; one row per account (`account`, `role`, `state`, `last_login_micros`, `used`, `limit`), the same in text and `--json`; no token field exists on the row. `admin user set-quota <account> <size>`: size parsed by the argument parser (`lots` exits 3), one `AdminService::set_quota(Operator, account, Some(limit))`, `NotFound` exits 2 with `not_found`. `admin audit export --since`: `--since` must be RFC 3339 UTC (exit 3 otherwise), `AuditReader::export` paged until no next page, each page written to stdout at once as JSON lines (`id`, `at_micros`, `event`, actor and subject ids, `detail` of ids, codes and integers); the stored client address is not printed. Composition root: new services `dyn AdminService` and `AuditReader`.

## Blocked

- AC-3 `admin user delete`: `AccountDeletionService::request(principal, confirmation)` takes a principal and the operator has none (spec concern, Medium, open, E-17 owner). The command stays `NotImplemented` (exit 2); inventing a principal would be a stand-in.
- AC-4 `admin dev-seed`: `DevSeed::seed` exists and passes its own suite, but the binary has no development `ProviderLogin` (mock-idp does not exist, spec concern in AZ-3822, Medium, open, E-22 / E-31 owners), so the command cannot be composed. Stays `NotImplemented`.

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- `audit export` prints no `ip`: the spec says "ids and action codes only", `AuditRow.ip` is an address. Recorded in Discovered #1.
- The `user list` loop ends on an empty page, so a real service makes one extra call after the last full page; a recording service that ignores `limit` is read correctly too.
- The page loop writes each page before reading the next, so a read failure after page 1 leaves page 1 on stdout and exits 2.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Test Suite

- `sunnycloud-server` crate (the only crate whose code changed): 184 passed, 0 failed, 0 ignored (`tests/binary.rs` now uses `admin user delete alice --yes` as its unfinished-command example).
- Workspace: not re-run this batch (focused tests per batch, one full-suite gate at the end of the implementation phase).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Is the client address part of "ids and action codes only" for `audit export`? | AZ-3795 AC-5 | Left out | unclear (an owner may want it for abuse review) |
| 2 | `--since` format | AZ-3795 concern 3 | RFC 3339 UTC like `--at` | clear |
| 3 | Operator form of account deletion; development `ProviderLogin` | AZ-3795 concerns 1, AC-4 | Blocked, ask | unclear |

## Commit

`[AZ-3795] Admin user list, set-quota and audit export commands` — Loop: 1

## Next Batch: cumulative review of batches 76-78, then remaining ready tasks (owner answers pending: AZ-3797, AZ-3794, AZ-3796 AC-6, AZ-3795 AC-3/AC-4, AZ-3793 AC-6, AZ-3809, AZ-4022)
