# Batch Report

**Batch**: 58
**Tasks**: AZ-4250_shared_test_support (done), AZ-3923_vault_header_service (done), AZ-3969_sync_core_wasm_vectors (done; UniFFI leg `NOT-RUN(EMU)`)
**Date**: 2026-11-13

Three parallel workers (component test suites + `sc_testdb`; `20_vault_service`; `sync-core-wasm` + a new CI script). The parent wrote the vault cascade migration, added the dependencies each task needed, approved one additive contract field (`VaultHeader.root_node_id`), moved the `AccountGuard` double into a new test-support crate after the gate showed the first placement broke the helper layering, updated one 02 test for the cascade, wrote every document change, reviewed, ran the gate and committed. Cumulative review of batches 55-57 written before the batch.

## Owner decisions 2026-11-13

1. A vault is removed with its root folder when 02 purges the tombstoned root after the retention window (migration `20261113120000_cascade_vault_on_root_purge`, `ON DELETE CASCADE`); the account can then create a new vault.
2. Batch 58 as proposed: a file as vault root answers `NotFound`; `get_header` also returns the root node id; the WASM module exports a commit-id maker that takes the time from the browser (as built it also takes the 10 random bytes from the caller, so the module stays pure and the vectors check the id).
3. After the batch: the account's own root folder is refused as a vault root even when empty (`NotFound`, checked through `TreeReader::root_node`; test `the_account_root_folder_is_not_found_even_when_empty_and_stores_nothing`). A 0-byte header stays accepted.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-4250_shared_test_support | Done | One statement capture (`sc_testdb::statements`) replaces five copies (01, 02, 05 feed, 05 via_tree, 08); one boundary-backed `StateGuard` (`sunnycloud-testkit`) replaces four copies (02, 03, 05, 08; also used by 20). 05's via-tree tests use the real 04 `GcQueue` (the `RecordingGcQueue` double is gone). `zeroize` declared once in the workspace (vault-core, 04, daemon). No assertion added or weakened |
| AZ-3923_vault_header_service | Done | `PgVaultService`: create (cap, root through one `TreeReader::list_children(limit 1)`: unknown, foreign, tombstoned or file root `NotFound`, non-empty root `VaultExists`; `INSERT ... ON CONFLICT DO NOTHING` so a second vault or root is `VaultExists`), get (header, ETag, root), replace decided by one conditional `UPDATE` on the SHA-256 of the stored bytes. AC-1..AC-8 on PostgreSQL 18: byte round trip after a restart, cap, root cases, stale ETag, 50 concurrent replaces (1 wins, 49 `PreconditionFailed`), account scope, one vault per account, and the real 02 delete + tombstone purge removing the vault so a new one can be created |
| AZ-3969_sync_core_wasm_vectors | Done, UniFFI leg not run | Exports `hash_block`, `rollup`, `new_commit_id(unix_ms, random10)`, `client_key_for_mobile`, `block_size()`, `UploadPlanner`; errors are JS `Error`s named by kind; 57 KB module with no I/O, clock or randomness. Vector file `src/sync-core-wasm/vectors/sync_core_vectors.txt` (9 KB, 30 cases) generated from native code only. Gate `scripts/ci/check-wasm-vectors.sh`: native, Node, Chromium, Firefox, WebKit PASS with 0 differences; module memory after the 100 MiB case equals that after 6 MiB; a seeded one-bit change fails every leg and names the case. AC-5 (UniFFI, Kotlin, emulator) `NOT-RUN(EMU)` |

## Parent changes

- Migration `20261113120000_cascade_vault_on_root_purge` (up, down, up checked on a copy of the test template).
- `contracts::VaultHeader.root_node_id` (part 3 1.3.0).
- New crate `sunnycloud-testkit` (`src/components/testkit`, dev-dependency only; its two statements in `account_repo.rs`). First placed in `sc_testdb`, which made the helpers reach `contracts` through their test dependencies (the `contracts` listing test and `sc_observability`'s crate-graph test failed); moved so neither rule needed an exception.
- 02 `lifecycle_vault.rs`: the "tree hook first is refused by the vault root key" test now proves the tree hook first removes the vault with its root (cascade).
- Dependencies: `sc_testdb` + `tracing`; 05 dev 04; 20 runtime `sc_clock`, `sc_ids`, dev 02, 04, 05, 07a; `sync-core-wasm` runtime `sync-core-proto`, `uuid`; 02, 03, 05, 08, 20 dev `sunnycloud-testkit`; workspace `zeroize`; 08's unused `tracing` dev-dependency removed. Lockfile: path and edge entries only.

## Contract and document changes (parent)

`data_model.md` §10 (`vaults` FK cascade) and the erase-order note; part 3 1.3.0; 20 description (root rules, cascade); AZ-3923 spec (decisions, AC-8); AZ-4250 spec (03's copy, testkit placement); `wasm_exports.md` 1.1.0 (as built, open point 1 closed, vector file, gate); `sync_core_proto.md` open point 4 closed; `scripts/ci/README.md` (wasm gate).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, wasm build and the four check scripts exit 0; workspace tests on PostgreSQL 18: 3353 passed, 1 failed, 21 ignored. The one failure is `sync-core` `hash_guards::a_share_of_five_percent_takes_much_longer_than_a_full_core_for_the_same_blocks` (timing: the full-core run took 68 ms against 192 ms under load from another project's build); `sync-core` is not touched by this batch and the test passed 3 of 3 runs alone, so it is recorded as timing-flaky under load, not blocking. The first gate run found the layering break above (2 tests, db-gates) and was fixed before the second. The WASM gate (`check-wasm-vectors.sh --require-browsers`) and its seeded test pass in the repository.

Warnings (recorded):
- The vault root's emptiness is checked by a port read before the insert, not in the insert's transaction: a child created in that instant can sit in a new vault (20 cannot read `nodes`; AZ-3927's `vault_id` copy is the place to close it).
- `report_unlock_failure` answers `Unexpected` until E-30/c04; its route must not be mounted before then.
- The ETag is PostgreSQL's `sha256` over the stored column (lower-case hex), not an `sc_ids` helper: `sc_ids` hashes only text tokens, and the conditional `UPDATE` must compare inside the database. 11 adds and strips the quotes.
- A 0-byte header is accepted (the account's own root folder is refused since owner decision 3).
- `check-wasm-vectors.sh` is not in a CI job yet (E-31); its test is not in `run-script-tests.sh` (needs cargo, wasm-pack and node).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | The account root as vault root and a 0-byte header were accepted | AZ-3923 | Owner decision 3: root refused, 0-byte header kept | clear |
| 2 | wasm-bindgen 0.2.130 cannot export a constant: `BLOCK_SIZE` is `block_size()` | AZ-3969 | Contract updated | clear |
| 3 | `client_key_for_mobile` exported so the key cases are compared | AZ-3969 | Contract updated | clear |
| 4 | A `contracts` edge from `sc_testdb` breaks the helper layering even as a test edge | AZ-4250 | `sunnycloud-testkit` | clear |

## Commit

`[AZ-4250] [AZ-3923] [AZ-3969] Test support, vault header, wasm vectors` — Loop: 1

## Next Batch

Recompute the ready tasks.
