# Cumulative Review — batches 45 to 47 (loop 1)

**Date**: 2026-11-12
**Range**: `f70de7e..2637188` (batch 45 `19a59e5`, batch 46 `5672a04`, batch 47 `2637188`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review with every finding fixed, decided by the owner or recorded. This pass looks across the three batches for drift that no single batch review would see.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds for normal and build edges. One rule change (owner, 2026-11-12): dev-dependencies between components are allowed. The tree tests use the real 05 stamp, and the check has a test for the accepted dev edge |
| Shared contracts changed | `TreeServiceError::AccountNotActive` (mapped by 11 to `account_not_active`, as on commit). No other contracts change. `BlobError::BlockConflict` is pending (batch 48) |
| Lockfile growth | About 90 packages: UniFFI (v1.2, accepted by the owner with six MPL-2.0 exceptions) and `keyring` 4.2.0 (required by the 14 description). Every v1.0 audit now scans them |
| One canonical path | No parallel implementation found. The worker id is resolved once per process; the clock poll reuses the API client type; the migrate commands make one `Migrator` call each |
| Test tiers | Performance suites run here only as information (gates `NOT-RUN(HW-REF)`). Security suites run at the component tier. Device and OS legs (Kotlin, Swift, Windows) are recorded as not run |
| Feature gates absent from release | `test-freeze` (decoder) and `test-clock` (worker) are checked by binary-content tests. The worker check uses the worker's own markers, because workspace feature unification puts `OffsetClock` into every test-profile binary |

## Open findings carried forward

| # | Finding | Owner / next step | Severity |
|---|---|---|---|
| 1 | `write_block` accepts a rewrite with different bytes under the same nonce (the 2026-11-11 rule is not implemented; `BlockConflict` is missing) | Batch 48 (owner decision) | High (security) |
| 2 | Device login wire mismatch between sync-core and the server; a locked keyring signs the user out | AZ-4231 | High (functional) |
| 3 | OpenAPI parameters without a pattern (`x-blake3`, `cursor`, `user_code`) | Follow-up in the route registry | Medium |
| 4 | `contracts::MasterKey` does not zeroize on drop | Contracts owner | Medium |
| 5 | IT-12-11 needs `sc_db` `test-migrations` with a directory-merging `Migrator` and the `sc_config` key | E-06 and E-01 tasks | Medium |
| 6 | `max_vault_header_bytes` default lives in the vault crate, not in `sc_config`'s key table | Follow-up | Low |
