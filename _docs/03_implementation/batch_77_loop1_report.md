# Batch Report

**Batch**: 77
**Tasks**: AZ-3796_epoch_restore_gc_commands (partly, stays in `todo/`)
**Date**: 2026-11-16

Picked from the ready set after batch 76 (AZ-3807 unblocked `epoch-bump`). AZ-3795, AZ-3809 and AZ-4022 were read and each has an open Medium owner concern (see Not started); AZ-3793's AC-6 needs work in helpers 07 and 01 that no task covers yet.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3796_epoch_restore_gc_commands | **Partly, stays in todo** (AC-1..5, AC-7 done; AC-6 blocked) | `maintenance_commands.rs`, `utc_arg.rs` (new), `dispatch.rs`, `doctor_command.rs`, `lib.rs`, `root/domain.rs`, `root/lists.rs`, `Cargo.toml` (+`chrono`, as 01 has it), `tests/maintenance_commands.rs`, `tests/gc_command.rs` | 9/9 (+ whole server crate 178/178) | See Blocked |

`admin epoch-bump [--yes]`: one `EpochAdmin::bump_epoch(clock.now_utc())`, then the `restore`-tagged checks through AZ-3807's `run_checks`; prints the epoch and every finding; exit 0 whatever the findings, exit 1 with `epoch_bump_failed` and the run-again advice when the bump returns an error. `admin restore`: account and `--at` (RFC 3339 UTC only, `Z` required) are validated before any call (exit 3), one `TreeRestore::restore_revision` with `NodeOrPath::Path`, prints revision and placement, `BeyondRetention`, `NotFound`, `FileMissing` exit 2 with their codes. `admin gc-run`: one `GcSweeper::run_cycle(now)`, prints `removed`, `remaining_due`, `budget_spent`. Composition root: new services `dyn EpochAdmin`, `dyn TreeRestore` and `GcSweeper` (the sweeper the janitor list held is now the same instance, built once).

## Blocked

AC-6 `admin restore --dry-run`: `TreeRestore::restore_revision` has no dry-run input (spec concern, Medium, open: E-13 owner). Running a real restore for `--dry-run` would be the opposite of what the operator asked. The command refuses `--dry-run` with exit 2, code `dry_run_unavailable`, no call, and says nothing was done. Needs an E-13 answer (a dry-run port form, or drop the option).

## Not started (open Medium owner concerns, no stand-in written)

- AZ-3795: `admin user delete` has no operator form of `AccountDeletionService::request(principal, ..)`; `admin dev-seed` cannot be composed without a development `ProviderLogin` (mock-idp does not exist). `user list`, `set-quota` and `audit export` have no blocker and are the next candidates.
- AZ-3809: `describe_blobs` lacks account and revision id; two missing-blob checks (E-10/c13 and this one) cannot both be registered; no task owns the providers or URL probes; levels of drift and clock findings unstated.
- AZ-4022: levels of the providers and setup findings unstated (decide whether a pre-setup `doctor` exits 0 or 1).
- AZ-3793: AC-6 (failing, killed, irreversible migrations) needs the test-migration mechanism of AZ-3787 built in helper 07 and helper 01; no task exists.

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- `gc-run` has one test over an empty queue on the real sweeper (counts zero, exit 0). A non-zero count and "exactly one `run_cycle` call" are not asserted: the sweeper is a concrete type with no port to record on, and seeding due blobs needs the account, blob and queue rows. Left as is rather than adding a test-only trait.
- The restore time argument uses `chrono` (already a dependency of 01) because the workspace has no RFC 3339 parser; `sc_clock::Timestamp` still has no formatter (batch 75 note).
- `--dry-run` is accepted by the parser and refused by the handler; if E-13 drops the option the argument goes too.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Test Suite

- `sunnycloud-server` crate (the only crate whose code changed): 178 passed, 0 failed, 0 ignored.
- Workspace: not re-run this batch (focused tests per batch, one full-suite gate at the end of the implementation phase). The batch 76 full run was 3760 passed, 3 load-sensitive failures that pass alone, 31 ignored.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `--dry-run` has no owner support | AZ-3796 concern 1 | Refused, blocked, ask | unclear |
| 2 | `FileMissing` mapped to exit 2 with code `file_missing` | AZ-3796 concern 3 | As the other owner refusals | clear |
| 3 | `Timestamp` has no RFC 3339 parser or formatter | AZ-3796 AC-4, AZ-3798 | `chrono` in the command crate for parsing; output stays Unix micros elsewhere | unclear (a shared helper in `sc_clock` is a small task) |

## Commit

`[AZ-3796] Epoch bump, restore and gc-run commands` — Loop: 1

## Next Batch: AZ-3795 (`admin user list`, `set-quota`, `audit export`), then other ready tasks; owner answers pending for AZ-3797, AZ-3794, AZ-3796 AC-6, AZ-3793 AC-6, AZ-3809, AZ-4022
