# Batch Report

**Batch**: 50
**Tasks**: AZ-4236_session_token_wire (done), AZ-3709_tree_publish_core (done), AZ-3925_vault_erase_hook (partly, stays in todo); new follow-up AZ-4238
**Date**: 2026-11-12

Three parallel workers on disjoint crates (`sync-core` + `sync-core-proto` + 11's identity tests, `02_tree`, `20_vault_service`). The parent wrote the contract changes (workers do not edit `_docs/`), fixed the crate-graph test helper, reviewed the code, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-4236_session_token_wire | Done | AC-1: `SessionTokens` is `{access_token, refresh_token, expires_in}` as the server answers both the mobile code exchange and the refresh; a refresh answer always carries a refresh token, so the old missing-token branch is gone. AC-2: `FakeAuth` and the sync-core fixtures answer the server's shape. AC-3: the real `HttpSyncClient` exchanges a mobile code and refreshes against the full 11 stack (`session_tokens_e2e.rs`, through the per-run TLS terminator now shared in `tls_terminator.rs`) |
| AZ-3709_tree_publish_core | Done | `TreeCommit::publish` on `PgTreeService` (which now takes the `QuotaLedger`): guard first, account advisory lock, effects' `on_begin` (replay), one savepoint per entry (node and revision writes, usage delta, reservation release, effects' `on_entry`), effects' `on_batch`, stamp over the union of touched nodes last, COMMIT, notify. Outcomes `Created`, `Updated`, `Replaced`, `NoChange`. AC-1..AC-10 pass on PostgreSQL 18 with the real quota ledger, blob registry and 05 stamp, including a 500-entry batch, 50 concurrent commits fed exactly once, and a statement recorder proving nothing runs between the stamp and COMMIT. `run_retrying_each` builds each attempt by value so the retry works inside an `async_trait` method |
| AZ-3925_vault_erase_hook | **Partly, stays in todo** | `PgVaultLifecycle`: `initialize` is a no-op (AC-1); `erase_batch` deletes up to `limit` rows in one short transaction and reports what is left (AC-2, AC-3, restart-safe). Open: AC-4 (erase not blocked by the root foreign key) needs 02's erase hook, AZ-3717 |
| AZ-4238_mobile_code_refusal (new) | Spec written, Jira AZ-4238 | The server refuses a bad mobile code with 400 `invalid_request` field `code`; sync-core only maps `invalid_code` to `InvalidCode` |

## Contract changes (parent)

`sync_core_proto.md` 1.1.0: the `SessionTokens` row names `access_token`, `refresh_token`, `expires_in`. `tree_commit.md`: the two 2026-11-12 decisions (Updated/Replaced, account id from the actor).

## Code Review Verdict: PASS

One finding, fixed by the parent: the vault erase hook kept its SQL text in `lifecycle.rs`, which the `check-db-gates` SQL-text gate refuses; the two statements moved to `vault_repo.rs`. No other findings. Checked: the savepoint split between entry failures and whole-call errors (`EntryError`), the stamp skipped when nothing was touched, the reservation released also for `NoChange`, every statement naming `account_id`, the retry wrapper's attempt-per-try, and the vault erase transaction staying short.

The crate-graph test `component_to_component_edge_is_rejected` failed once the tree crate really dev-depended on `sunnycloud-quota`: the helper `add_edge` seeded a normal edge without dropping the matching dev-only entry, so the fake edge was skipped. Fixed in `server/tests/crate_graph.rs`; 11 of 11 pass.

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3005 passed, 0 failed, 19 ignored), wasm build and the four check scripts exit 0 (`check-db-gates` after the `vault_repo.rs` move, rerun with the vault crate's tests, clippy and fmt).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | A stale base, a taken name and a tombstoned file roll the whole call back with `OutcomePending` (unexpected tier) until AZ-3710 adds `ConflictCopied`, `CreatedAsCopy` and `Revived` | AZ-3709 | As implemented; AZ-3710 replaces it | clear |
| 2 | An unknown blob and another account's blob fail the same foreign key and answer the per-entry `BlocklistMismatch` | AZ-3709 AC-8 | As implemented | unclear |
| 3 | `NoChange` releases the entry's reservation and writes nothing | AZ-3709 | As implemented | clear |
| 4 | An `Existing` target naming a folder answers `NotFound` | AZ-3709 | As implemented | unclear |
| 5 | The usage delta of `Updated` and `Replaced` is 0 until AZ-3711 | AZ-3709 | AZ-3711 | clear |
| 6 | Invalid mobile code answer mismatch (server `invalid_request` field `code`, sync-core `invalid_code`) | AZ-4236 | New task AZ-4238 | clear |
| 7 | `tree` dev-depends on `sunnycloud-quota` and `sunnycloud-blobstore` (allowed dev edges, ADR-015) | AZ-3709 | As implemented | clear |

## Commit

`[AZ-4236] [AZ-3709] [AZ-3925] Session token wire, tree publish core, vault erase hook` — Loop: 1

## Next Batch

Recompute the ready tasks; AZ-4238 (1 point) and AZ-3710 (publish conflict outcomes) are candidates.
