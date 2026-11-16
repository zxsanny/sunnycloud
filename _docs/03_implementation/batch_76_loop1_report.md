# Batch Report

**Batch**: 76
**Tasks**: AZ-3807_doctor_aggregation, AZ-3822_admin_dev_seed (archive only)
**Date**: 2026-11-16

AZ-3807 is the only code change of the batch. AZ-3822's code and tests were committed in batches 65 and 71; this batch checked its three acceptance criteria against the passing `dev_seed` suite (3/3) and archived the spec. AZ-3794 was in the plan and stays in `todo/`: see Blocked.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3807_doctor_aggregation | Done | `doctor_command.rs` (new), `dispatch.rs`, `lib.rs`, `output.rs`, `tests/doctor_command.rs` (new), `tests/exit_codes.rs` | 6/6 | None |
| AZ-3822_admin_dev_seed | Done (archived, no code change) | none | `dev_seed` 3/3 | Mock-idp concern stays with the `admin dev-seed` command task |

AZ-3807: `doctor` runs every registered `DoctorCheck` in registration order (`run_checks`, public, with `CheckSelection::{All, Tagged(tag)}` so `admin epoch-bump` can run the `restore` subset), builds the `DoctorContext` from `--pre-flight`, `--backup-interval`, `--attest-backup`, and returns one `findings` list (check, level, message, remedy). Worst level gives the class: Fail exit 2, Warn exit 1, otherwise 0. `--pre-flight` selects only `pre-flight`-tagged checks. The text writer now prints a list value one element per line, so text and `--json` carry the same content. A malformed `--backup-interval` is rejected by the argument parser (exit 3) before any check runs.

## Blocked

AZ-3794 `admin setup-token|create|reset|invite|promote|demote`: the spec's flagged concern (Medium, open) is not answered: `OperatorIdentityOps::create_admin_invite()` takes no note and no TTL, while the command takes `--note` and `--ttl`. Dropping them, or storing them elsewhere, would be a stand-in. Needs an owner or E-15 answer (extend the port, or remove the two options from the command surface).

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- `--pre-flight` does not yet avoid the database in the real binary: `root::DoctorChecks` is built with a connected `Db` (`Db::connect` opens its pool eagerly) and `KeyDoctorCheck::new` needs one. The aggregation selects only `pre-flight` checks and builds nothing else, which the harness test proves, but the owner-built list still connects. Fix needs 04's `KeyDoctorCheck` (or `sc_db`) to accept a lazy database. See Discovered #1.
- A check that panics or hangs is not isolated and has no timeout (spec concern, Low, open).
- A `doctor` run writes no audit row (spec concern, Low, open); it is read-only.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Test Suite

- Total: 3794 tests
- Passed: 3760 in the full run; 3 more after an isolated rerun (see below), so 3763
- Failed: 0 after the rerun
- Skipped: 31 (ignored, unchanged)

`cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18 (`sunnycloud-pg18`, `max_connections=400`). The full run had 3 failures in code this batch did not touch, all load-sensitive; each passed alone with `--test-threads=1`:
- `http-api` `sharing::uniform::every_missing_expired_revoked_or_deleted_link_answers_the_same_404`: compares two answers including the `date` header; the clock moved one second between the requests.
- `sharing` `visitor_password::a_token_that_is_not_a_live_password_link_is_not_found_at_once_without_a_permit`: median 3.5 ms against the not-found budget.
- `tree` `publish_concurrency::fifty_concurrent_commits_feed_each_seq_once_in_order_one_hundred_times`: `Unexpected(Unavailable)` from the database under 50 concurrent publishes while another test run shared the machine.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `doctor --pre-flight` with the database stopped still connects in the real composition: `DoctorChecks` needs `Db`, `Db::connect` is eager | AZ-3807 AC-3 | Make `KeyDoctorCheck`'s database lazy (04) or give `sc_db` a lazy pool; then register the pre-flight list without a connection | unclear (touches 04 or `sc_db`, owner choice) |
| 2 | `create_admin_invite` has no note or TTL | AZ-3794 concern 2 | Blocked, ask | unclear |
| 3 | Three load-sensitive tests failed in the full run and pass alone (listed under Test Suite) | none of this batch's files | Report only; a tolerance for the `date` header in the uniform-404 comparison would remove the first | clear |
| 4 | Per-check timeout and panic isolation in `doctor` | AZ-3807 concern 1 | Not added | unclear (owner may want 07's per-check timeout) |

## Commit

`[AZ-3807] [AZ-3822] Doctor aggregation, dev seed archived` — Loop: 1

## Next Batch: remaining `todo/` tasks in dependency order (AZ-3793 migrate commands, AZ-3809 doctor root checks, AZ-3796 epoch/restore/gc commands, then others)
