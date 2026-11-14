# Cumulative Review — batches 67 to 69 (loop 1)

**Date**: 2026-11-14
**Range**: `fc5422f..b173fd0` (batch 67 `3dcb3c4`, batch 68 `eddd09f`, batch 69 `b173fd0`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review; findings were fixed or recorded. This pass looks across the three batches. 186 files changed, about 8,900 lines added: vault membership across 02, 03, 06, 11 and 20 (one migration); the account deletion purger (07) with `lock_deleting` (01); sharing and transfer security and integration suites; admin security, acceptance and hook-chain suites.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` green in every gate). No new normal component edge; 02, 03 and 06 have no normal dependency on 20 and never call `VaultService` |
| Shared contracts changed | Additive only: `VaultRoots`, `NodeView.vault`, `LiveFile.vault`, `UploadError` / `CommitServiceError::VaultFileTooLarge`, `TreeServiceError::VaultBoundary` (part 2 1.3.0, part 3 1.5.0); `AccountAdmin::lock_deleting`, `AccountInfo.deletion_requested_at` (part 1 1.6.0) |
| Error codes | `vault_file_too_large` 413 (owner) and `vault_boundary` 409 (added without an owner decision) |
| Config table | `account_delete_cycle_budget` 5 s |
| Migrations | One: `20261114100000_add_tree_node_vault_membership` (02 column, 20 key); every component's up/down/up test passes; 05's feed test now isolates the feed migration's own objects |
| Lockfile | No new external package |
| System-scope allowlist | One test-only line (03 `real_account_repo.rs`) |
| Production code quality | No `unwrap()` and no `#[allow]` added in production sources; no SQL `now()` (gate enforced; two test inserts fixed) |
| File length | No file over 500; close: `02 src/publish.rs` (488), `03 tests/transfer/start_support.rs` (495) |
| Test stability | Two test-side defects fixed: a timing-dependent slow-consumer read-ahead test, then a race in that fix which could hang the reader binary |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | Vault edge cases: a tombstoned file revived inside a vault has no membership; operator restore can place a vault file in the plain `Restored` folder | Owner (E-13/E-30) | Medium |
| 2 | `sync-core` maps `vault_file_too_large` and `vault_boundary` to "unexpected" | 13 (client) owner | Low |
| 3 | 11 HTTP gaps from batch 66 still open (`ClientKeyMismatch` 500, `overloaded` without `Retry-After`, route error lists) | E-21 | Medium |
| 4 | The purger re-calls 02 until 04's GC runs (`gc_interval` 5 min) | E-22 scheduling (AZ-3789) | Low |
| 5 | `02 src/publish.rs` at 488 lines | Split by responsibility when it next grows | Low |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | OpenAPI parameters without a pattern | Open |
| 2 | `contracts::MasterKey` does not zeroize on drop | Open |
| 3 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 4 | `max_vault_header_bytes` default outside `sc_config` | Open, Low |
| 5 | Timing-flaky under load: 07a lock wait, sync-core `hash_guards`, server `config_binary` | Open, Low |
| 6 | Generated Kotlin and Swift bindings print token fields | Open |
| 7 | Android JNI and UniFFI legs never run | Leftover |
| 8 | `rust-lint.sh` steps 3-4 need `psql` here | Environment |
| 9 | Test log capture copied in several suites | Shared helper when next needed |
| 10 | Safari S12 run; REF-SRV timing rows | Leftovers |
| 11 | Owner questions: AZ-3755 suffix ranges, AZ-3700 AC-6, AZ-3698 pending-cascade timing, AZ-3738 foreign base, AZ-3701 unknown token on the password route, AZ-3927 items, AZ-4024 flags | Owner |
