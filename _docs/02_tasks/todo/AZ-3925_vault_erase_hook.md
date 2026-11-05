# `AccountLifecycleHook` for vaults: no-op initialize, `erase_batch` removes vault rows

**Task**: AZ-3925_vault_erase_hook
**Name**: Vault erase hook
**Description**: Implement 20's `AccountLifecycleHook`: `initialize` does nothing and `erase_batch(account, limit)` deletes the account's `vaults` rows in short batches and reports how many remain
**Complexity**: 2 points
**Dependencies**: AZ-3922_vaults_migration_config, AZ-3768_contracts_part1, AZ-3782_sc_db_access
**Component**: 20 vault_service [v1.2]
**Tracker**: AZ-3925
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `AccountLifecycleHook`, `EraseProgress` (E-00/c03).
- `_docs/02_document/contracts/shared_db/sc_db_access.md` — account scope and short transactions (E-06/c01a).

## Problem

Account deletion (AC-27) must leave no vault rows. 07's `DeletionPurger` drives every component's `erase_batch` in short batches until each reports `remaining = 0` (data model §12). 20 owns `vaults`, so it must provide the hook. Its vault rows reference the root node, so its hook runs before 02's (owner decision 2026-11-05).

## Outcome

- After erase, the account has 0 `vaults` rows and the hook reports `remaining = 0`; other accounts are unchanged.
- Repeated or interrupted erase runs finish without error.

## Scope

### Included
- `initialize(tx, account)`: no write, no error.
- `erase_batch(account, limit) -> EraseProgress{remaining}`: delete up to `limit` of the account's `vaults` rows in one short transaction and return the remaining count.

### Excluded
- Registering the hook in 12's list at its position, before 02's hook (E-30/c06 with E-22/c03).
- Deleting vault ciphertext: vault files are ordinary nodes and revisions, erased by 02 and 04 (IT-20-8 checks the result in E-30/c08).

## Acceptance Criteria

**AC-1: Initialize is a no-op**
Given account creation calling every hook's `initialize` in one transaction
When 20's `initialize` runs
Then it writes nothing and returns without error

**AC-2: Erase leaves zero rows (AC-27)**
Given account A with its vault rows and account B with its own vault
When `erase_batch(A, 5000)` runs until it reports `remaining = 0`
Then A has 0 `vaults` rows and B's vault row and header are unchanged

**AC-3: Idempotent and restart-safe**
Given A's vault rows already erased, or an erase interrupted between two batches
When `erase_batch(A, limit)` runs again
Then it returns `remaining = 0` after the remaining rows are gone and never fails on rows that no longer exist

**AC-4: Erase is not blocked by the root foreign key**
Given the hook order of 12's list, with 20's hook before 02's (owner decision 2026-11-05), and the `vaults.root_node_id` foreign key (E-30/c02)
When the whole purge of an account with a vault runs (02's erase of nodes included)
Then every hook reaches `remaining = 0` and no batch fails on the foreign key

## Non-Functional Requirements

**Reliability**
- Each batch is its own short transaction; no transaction spans batches (data model §12, "never one long transaction").

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `initialize` with a recording transaction | No statement issued |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Real PostgreSQL 18; accounts A and B with vaults | Erase A to `remaining = 0` | A: 0 rows; B unchanged | IT-20-8, AC-27 |
| AC-3 | A already erased; erase stopped mid-way | Run again | `remaining = 0`; no error | NFT-RES-30 |
| AC-4 | Account with a vault on a root folder | Full hook list run (real 02 hook) | All hooks `remaining = 0`; no FK error | FT-P-19 |

## Constraints

- Only 07 calls `erase_batch`; only 01 calls `initialize` (canonical-names § Ports).

## Risks & Mitigation

**Risk 1: Foreign key versus erase order**
- *Risk*: 02 deletes the root node while the vault row still references it.
- *Mitigation*: 20's hook runs before 02's (owner decision 2026-11-05); AC-4 proves it with the real 02 hook.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Data model §12 runs 20's hook after 02's; with `vaults.root_node_id` referencing `nodes`, either the hook moves before 02 or the key gets a delete rule (same row on E-30/c02) | E-30 / E-13 / E-22 owners | resolved | High |
| Resolved by owner decision 2026-11-05: 20's erase hook runs before 02's (production order 06, 20, 02, 03, 08, then 05, 07a); no delete rule on `vaults.root_node_id` is needed | Owner | resolved | High |
