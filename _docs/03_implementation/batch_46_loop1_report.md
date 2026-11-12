# Batch Report

**Batch**: 46
**Tasks**: AZ-3693_identity_scope_declarations_check (done), AZ-3793_migrate_commands (partly, stays in todo), AZ-4007_vault_core_skeleton (partly, stays in todo)
**Date**: 2026-11-12

Three parallel workers on disjoint crates (`11_http_api` tests, `server`, `vault-core`); only the vault-core worker could change `Cargo.lock`. The parent wrote the scope table under `_docs/`, applied the owner's four decisions, ran the gates and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3693_identity_scope_declarations_check | Done | `tests/it/registry/scopes.rs`: reads `x-owner-method` and `x-auth-class` from the generated OpenAPI document and fails, naming the operation and its expected class, when a declaration differs from 01's table or an expected owner is missing; the four AC-57 operations must be `SessionOnly` with no device bearer. The table is written from 01 description section 3 (01 exposes none) and kept beside the authenticator contract (`contracts/01_identity/authenticator.md` 1.1.0). `DELETE /devices/me` is checked as `DeviceOnly` (owner decision). No declaration disagreed with 01's model |
| AZ-3793_migrate_commands | **Partly, stays in todo** | `migrate`, `migrate status`, `migrate revert` in the `sunnycloud` binary: one `Migrator` call each, refusals exit 2 with stable codes, `revert` needs `--yes` on a non-terminal, `migrate` and `revert` need the migrator secret, `status` uses it when set and otherwise the API URL, no master key needed. Tested on PostgreSQL 18 with the real binary: run and rerun, two concurrent runs apply each version once, edited checksum, unknown newer, dirty row, revert guard, role rules. **AC-6 (IT-12-11) blocked**: helper 07 (`sc_db`) has no `test-migrations` feature and no `Migrator` that merges a directory with the embedded set (`MigrationSet` is crate-private), helper 01 has no `test_migrations_dir` key, and `serve` is not implemented yet |
| AZ-4007_vault_core_skeleton | **Partly, stays in todo** | `vault-core` as a `no_std` pure library: exact pins (chacha20poly1305 0.11.0, aead-stream 0.6.0, argon2 0.6.0, blake3 1.8.7), the injected `RandomSource` (`OsRandom` only behind `os-rng`), the 19 `VaultError` variants checked against `vault_core_api.md`, format and bound constants, features `passphrase-policy`, `os-rng`, `uniffi`, `uniffi-cli`, `wasm`; crate-graph checks (no sunnycloud crate, no `contracts`, no `getrandom` in the WASM tree); the iOS `cargo deny` licence check; a v1.2 CI matrix job (native, wasm, uniffi). **Not done**: the generation-overflow error variant (the contract leaves its name open); the Kotlin JVM smoke test (no `kotlinc` here); the Swift leg (`NOT-RUN(H-MAC)`) |

## Owner decisions applied (2026-11-12)

1. `DELETE /api/v1/devices/me` stays `DeviceOnly`: 01 description sections 2 and 3, the scope table and the surface row updated; the check covers the route with 11's class directly (01's `Scope` has no device-only value).
2. `migrate status` exits 2 for a dirty version, a checksum mismatch or an unknown newer version (was: unknown newer only): code, unit and integration tests, the 12 description and the command surface updated.
3. UniFFI accepted now: about 50 packages in the lock and six crate-scoped MPL-2.0 exceptions in `deny.toml` (uniffi, uniffi_core, uniffi_internal_macros, uniffi_macros, uniffi_meta, uniffi_pipeline).
4. `wasm-bindgen` allowed in vault-core's WASM tree: the `wasm` feature added (it exposes nothing yet; the exports are E-29/c10's) and the server crate-graph test's allowlist names `vault-core` next to `sync-core-wasm`.

## Code Review Verdict: PASS_WITH_WARNINGS

Three findings: the `migrate status` exit code (decided by the owner, fixed); the passphrase minimum hard-coded in the `TooShort` message (fixed: `PASSPHRASE_MIN_CHARS` constant); the UniFFI dependency surface (accepted by the owner).

CI-parity: the full gate (fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18, wasm build, the four check scripts) ran green on the batch before the decisions (2844 passed, 0 failed, 9 ignored) and again on the final code before the commit: every step exit 0, 2844 passed, 0 failed, 9 ignored. Not run: `check-migrations.sh` (needs `psql` and `sqlx-cli`) and the workspace `cargo deny` (known RUSTSEC-2026-0459; the vault-core licence check passed).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | Stable error codes of `migrate` are not in helper 02's table: `migration_failed`, `checksum_mismatch`, `unknown_newer`, `irreversible_migration`, `migration_dirty`, `wrong_role` | contract open point 3 | Used as the command's own codes | unclear |
| 2 | `MigrationError::IrreversibleMigration` carries no raised text, so the literal `irreversible: restore from backup` cannot be printed | AZ-3793 | Prints the typed name and version | unclear |
| 3 | helper 07 and helper 01 work for IT-12-11 (feature, merging `Migrator`, `test_migrations_dir` key) | AZ-3787 follow-ups | Needs tasks in E-06 and E-01 | unclear |
| 4 | Per-package clippy (`cargo clippy -p sunnycloud-http-api -p sunnycloud-server`) fails in the untouched `components/04_blobstore/src/gc_sweeper/mod.rs:133` (`default_constructed_unit_structs`); the workspace clippy CI runs passes | none | Pre-existing, outside this batch's files; reported, not fixed | unclear |
| 5 | zxcvbn on wasm32 pulls `wasm-bindgen`, `web-sys` and `chrono` | 19 section 5 "zero imports" | For E-29/c05's size work | unclear |
| 6 | uniffi 0.32.2 pulls `tempfile` and `getrandom` through `uniffi_pipeline` | AZ-4007 AC-1 | The uniffi tree is checked for sunnycloud and network crates only | unclear |
| 7 | WASM and mobile Argon2 memory caps are not set (only the native 512 MiB cap) | E-29/c02 | Waits for the Argon2 benchmark (AZ-4005, blocked on devices) | clear |
| 8 | `uniffi-cli` feature and a `uniffi-bindgen` binary added beyond the spec, to generate the bindings in CI | AZ-4007 | Kept; requires the `uniffi-cli` feature | clear |

## Commit

`[AZ-3693] [AZ-3793] [AZ-4007] Identity scope check, migrate commands, vault-core skeleton` — Loop: 1

## Next Batch

Ready next: AZ-3908 (desktop token store and device login; adds the `keyring` crate). Five-point tasks wait for the owner's complexity-budget choice: AZ-3708, AZ-3747, AZ-3834, AZ-3916.
