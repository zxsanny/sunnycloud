# Batch Report

**Batch**: 80
**Tasks**: AZ-3807 follow-up (`doctor --pre-flight` without a database); no new ticket, AZ-3807 stays in `done/`
**Date**: 2026-11-17

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3807 AC-3 in the real binary | Done | 04 `keyring/doctor.rs`, `root/lists.rs`, `root/mod.rs`, `doctor_command.rs`, `tests/doctor_command.rs`, `tests/doctor_preflight.rs` (new), `tests/composition_root.rs` (+1), 04 `tests/maintenance/doctor.rs` (+1) | see Test Suite | None open |

Owner decision of 2026-11-17 (asked in this session): a second, database-free check list. `root::PreFlightChecks` is built from `ServeSettings` only (`VolumeCheck` and `KeyDoctorCheck::without_database`); `doctor --pre-flight` takes that list and never asks for `DoctorChecks`, `Blobstore` or `Db`. `KeyDoctorCheck` now holds `Option<Db>`: `new(config, db)` is unchanged, `without_database(config)` is the new constructor, and a lost `.keycheck` is judged only when a database is present and the run is not a pre-flight. A wrong key, a missing file or a wrong length still fail without a database.

The two lists must agree on the pre-flight checks: `composition_root.rs` asserts that `PreFlightChecks` holds exactly the checks of `DoctorChecks` tagged `pre-flight`. `tests/doctor_preflight.rs` runs the real binary with the database URL pointing at a listener and asserts that nothing dials it and that `master_key` and `blob_volume` are reported.

## Code Review Verdict: PASS

No findings. The only cost is the one the owner accepted: the pre-flight checks are named in two places, guarded by the composition-root test.

## Test Suite

- `sunnycloud-server` and `sunnycloud-blobstore` (the two crates whose code changed), `--no-fail-fast`: 541 passed, 1 failed. The failure, `rewrap::ids_the_ring_does_not_hold_or_that_are_equal_are_refused_and_zero_rows_is_zero`, panicked in shared test support (`scrub/support.rs:230`, a catalog lookup) during a run that overlapped another project's builds on this host; it and the other 6 rewrap tests pass when run alone. Recorded as load-sensitive, unrelated to this change (rewrap code is untouched).
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.
- Workspace: not re-run this batch (one full-suite gate at the end of the implementation phase).

## Commit

`[AZ-3807] Run doctor --pre-flight without a database` - Loop: 1

## Next Batch: remaining owner questions (AZ-3794, AZ-3796 AC-6, AZ-3795 AC-3/AC-4, AZ-3793 AC-6, AZ-3809, AZ-4022) or the unread small tasks (AZ-3824, AZ-3835, AZ-3926, AZ-3971)
