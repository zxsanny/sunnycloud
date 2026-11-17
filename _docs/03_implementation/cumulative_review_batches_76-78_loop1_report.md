# Cumulative Review — batches 76 to 78 (loop 1)

**Date**: 2026-11-17
**Range**: `cf4a300..HEAD` (batch 76 `1692de0`, batch 77 `5784988`, batch 78 `a54acc9`, fix `747965e`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review. This pass looks across the three: `doctor` aggregation, the maintenance commands (`admin epoch-bump`, `restore`, `gc-run`) and the account commands (`admin user list`, `set-quota`, `admin audit export`).

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds. New edge: `chrono` (no default features) in the server crate, same use as 01_identity |
| One implementation per port | `dyn AdminService`, `dyn EpochAdmin`, `dyn TreeRestore` are registered in the root over the instances 01, 02 and 05 already hold; `GcSweeper` and `AuditReader` are shared with `serve` |
| Shared contracts changed | None |
| Migrations | None |
| Production code quality | No `unwrap()` added in production sources; failed best-effort calls are logged |
| Gates | fmt, clippy (also with `vault`), `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` green per batch; server crate 184 passed, 0 failed |
| Test stability | Batch 76 full suite: 3 load-sensitive failures (uniform-404 `date` header, visitor-password median budget, tree publish concurrency), each passing alone; none touch code from these batches |

## Findings

| # | Finding | Outcome |
|---|---|---|
| 1 | `epoch-bump` built the restore checks after the bump had committed, so a failed build left a bumped epoch and no report | **Fixed** in `747965e` (checks built before the bump) |
| 2 | `doctor --pre-flight` still connects to the database in the real composition (`DoctorChecks` builder uses eager `Db::connect`) | Skipped: needs a 04 / `sc_db` owner decision |
| 3 | `user list` loops over pages until an empty page with no upper bound | Skipped: low risk on an operator command; revisit with the first real dataset |
| 4 | `restore --dry-run` is advertised in `--help` but refused | Skipped: AZ-3796 AC-6 owner question (port has no dry-run form) |
| 5 | RFC 3339 parsing belongs in `sc_clock` rather than `utc_arg` | Skipped: helper change outside these tasks |
| 6 | Output rows are built as hand-written JSON maps | Skipped: matches the existing command modules |
| 7 | New tests lack Arrange/Act/Assert comments | Skipped: to be added in one pass over the 76–78 tests |
| 8 | `audit export` leaves out the client address | No change needed: spec lists no address field |

## Carried forward

Owner questions from the earlier reviews are unchanged: AZ-3797, AZ-3794, AZ-3795 AC-3 and AC-4, AZ-3793 AC-6, AZ-3809 and AZ-4022.
