# Batch Report

**Batch**: 85
**Tasks**: AZ-3793_migrate_commands (AC-6 only; AC-1..AC-5 were an earlier batch), now complete
**Date**: 2026-11-17

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3793 AC-6 | Done (irreversible case waived at binary level) | `server/tests/migrate_interrupted.rs` (new) | 2 new | None open |

Owner decision of 2026-11-17 (asked in this session): no test-only migration source. The documented alternative (feature `test-migrations`, key `SUNNYCLOUD_TEST_MIGRATIONS_DIR`) would have added a feature, a config key and directory-merging code to `sc_db`, `sc_config` and the server, plus a release-image CI check and a test-stack mount that no task owns, for tests only. The faults are arranged from outside the binary instead.

- **Failing:** `audit_log` is created by hand before `migrate`, so the second shipped migration (`20261105100000`) fails on its first statement. `migrate` exits 2 with code `migration_failed` naming that version; the first migration stays applied (1 row). After the table is dropped, the next run applies every version from the failed one on and exits 0.
- **Killed:** the newest migration is made pending with the real `migrate revert --yes`; a lock on `vaults` held by the harness keeps the next `migrate` waiting inside it; the harness SIGKILLs the binary, releases the lock and waits for the migrator backend to end. The column the migration adds is not there (rolled back), `migrate status` lists the version as pending, `serve` exits 2 naming `pending` before any listener accepts, and a later `migrate` applies it and exits 0 (so the advisory lock was released).
- **Irreversible `down`:** no shipped migration has one, so there is nothing to meet through the binary. It stays covered by `sc_db` (`IRREVERSIBLE_DOWN`) and by the exit mapping unit test in `migration.rs`. This is the waived part of AC-6, recorded as the owner's decision.

Docs: AZ-3793 spec (AC-6 wording and G-12-1), 12 description §4 (marks the feature decision as superseded), `spec-gaps.md` G-12-1.

## Code Review Verdict: PASS

No findings; tests only.

## Test Suite

- `migrate_interrupted` 2 and `migrate` 9 passed, three runs in a row (stable, ~1 s each). The rest of `sunnycloud-server` was last run in batch 84 (220 passed) and no server source changed since.
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Commit

`[AZ-3793] Failing and killed migrations through the binary` - Loop: 1
