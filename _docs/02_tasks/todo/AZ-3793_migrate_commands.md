# `migrate run|status|revert`

**Task**: AZ-3793_migrate_commands
**Name**: `migrate` commands
**Description**: The one-shot `migrate` step and its `status` and `revert` forms: one call on helper 07's `Migrator`, the result printed, and the refusals mapped to the documented exit codes
**Complexity**: 3 points
**Dependencies**: AZ-3787_migration_test_source_spike, AZ-3788_cli_skeleton, AZ-3790_per_command_config, AZ-3783_sc_db_migrator_accounts_base, AZ-3742_test_harness_skeleton
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3793
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_db/sc_db_migrations.md` — `Migrator::run_pending`, `status`, `revert`, `MigrationError`.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — `migrate` rows (E-22/c02).

## Problem

Upgrades and restores run `migrate` as a separate Compose step before the API starts (S4-2). The transaction per migration, the advisory lock and the checksums are helper 07's. The operator still needs a clear result: what was applied, which version failed and why, and an exit code that stops the Compose chain. `revert` is for CI and recovery only and must not run by accident.

## Outcome

- `migrate` and `migrate run` call `Migrator::run_pending` with `db_migrator_url` and print the applied versions; a failure exits 2 naming the failed version and its error; a checksum mismatch exits 2 naming the version; an unknown newer database exits 2.
- `migrate status` calls `Migrator::status` with either role and prints applied, pending, dirty and unknown-newer; unknown-newer exits 2.
- `migrate revert [--steps N] --yes` calls `Migrator::revert`; without `--yes` on a non-TTY it exits 3; `IrreversibleMigration` exits 2 and stops.
- `migrate` needs no master key and no other section (E-22/c04).
- With the mechanism decided by E-22/c01, the failing, killed and irreversible cases of IT-12-11 run in the test image.

## Scope

### Included
- The three command forms, their output and exit mapping.
- The IT-12-11 cases through the E-22/c01 mechanism (or the part of it that lands in E-22).

### Excluded
- Transactions, the advisory lock, checksums, the `-- no-transaction` rule (helper 07, E-06/c01b).
- Migration content (each owner), the Compose `migrate` service (E-00/c11), the fresh-install flow (AZ-3654, E-22/p01), the missing-secret refusal (E-22/c04 AC-4).

## Acceptance Criteria

**AC-1: Run and rerun (IT-12-10)**
Given a fresh database
When `migrate` runs twice
Then the first run applies every migration, prints their versions and exits 0, and the second applies nothing and exits 0

**AC-2: Concurrent runs (IT-12-10)**
Given two `migrate` processes started at the same moment
When both finish
Then each migration version was applied exactly once and both exit 0

**AC-3: Refusals (IT-12-10)**
Given, in turn, an applied migration whose checksum the harness changed, a migrations row with a future version, and a row marked unsuccessful
When `migrate` and `migrate status` run
Then the checksum case exits 2 naming the version, the future version makes `status` exit 2 and report `unknown-newer`, and the unsuccessful row is reported as `dirty`

**AC-4: Revert guard (IT-12-10, ST-12-2)**
Given an applied migration
When `migrate revert --steps 1` runs on a non-TTY without `--yes`, then with `--yes`
Then the first exits 3 and changes nothing, and the second reverts exactly one migration and exits 0

**AC-5: No master key needed (IT-12-10)**
Given the master key file missing
When `migrate` runs
Then it succeeds

**AC-6: Failing, killed and irreversible migrations (IT-12-11, NFT-RES-14)**
Given the test migration source chosen by E-22/c01 with a failing statement, a long statement and a `down` that raises `irreversible: restore from backup`
When `migrate` meets the failing statement, is killed with `kill -9` during the long one, and `migrate revert --yes` meets the irreversible `down`
Then the failure exits 2 naming the failed version with earlier migrations still applied and the next run retrying from it; after the kill the migration is rolled back and `serve` refuses to start; the irreversible `down` exits 2 with `IrreversibleMigration` and stops

## Non-Functional Requirements

**Reliability**
- `migrate` exits non-zero on every failure so the Compose chain never starts `api` after a failed step.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Mapping of each `MigrationError` and status flag | Exit 2 with the version; `dirty`, `unknown-newer` printed |
| AC-4 | Guard on `revert` | No call without `--yes` on non-TTY |
| AC-6 | Mapping of `IrreversibleMigration` | Exit 2, stop |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Fresh PostgreSQL 18 | Two runs; two concurrent runs (IT-12-10) | Applied once; exit 0 | AC-65 |
| AC-3 | Edited checksum; future row; unsuccessful row | `migrate`, `status` (IT-12-10) | Exit 2 naming the version; `unknown-newer`; `dirty` | |
| AC-4 | One applied migration | `revert` without and with `--yes` | Exit 3 no change; one reverted | ST-12-2 |
| AC-5 | No master key file | `migrate` | Exit 0 | |
| AC-6 | Test migration source (E-22/c01) | Fail, kill, irreversible (IT-12-11) | Exit 2; rolled back; `serve` refuses; stop on irreversible | NFT-RES-14, FT-N-39 |

## Constraints

- Real PostgreSQL 18 with both roles; the migrator is a helper, not a port, so it is not replaced by a double.
- `migrate` uses only `db_migrator_url`; the API role cannot run it (ST-12-4).

## Risks & Mitigation

**Risk 1: IT-12-11 stays blocked**
- *Risk*: E-22/c01 picks a mechanism that lives in helper 07 and E-06 has not built it yet.
- *Mitigation*: AC-6 is the only blocked part; AC-1..AC-5 do not wait for it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-12-1: AC-6 is BLOCKED until E-22/c01 records the test-only migration source | E-22/c01 | open | Medium |
| Stable codes for checksum mismatch, unknown-newer and `IrreversibleMigration` are not in helper 02's code table (contract open point 3) | E-02 / E-22 owner | open | Low |
