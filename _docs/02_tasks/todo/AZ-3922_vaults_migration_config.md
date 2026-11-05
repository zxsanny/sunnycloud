# `vaults` migration, row-level policy and vault config keys

**Task**: AZ-3922_vaults_migration_config
**Name**: Vaults migration and config
**Description**: Add the additive v1.2 migration for the `vaults` table under the standard tenant row-level policy, and the typed config keys `max_vault_file_bytes` and `max_vault_header_bytes`
**Complexity**: 2 points
**Dependencies**: AZ-3913_vault_decisions_spike, AZ-3783_sc_db_migrator_accounts_base, AZ-3706_tree_skeleton, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3652_initial_structure
**Component**: 20 vault_service [v1.2]
**Tracker**: AZ-3922
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_db/sc_db_migrations.md` — reversible migration pairs and the migrator role (E-06/c01b).
- `_docs/02_document/contracts/shared_db/sc_db_access.md` — account scope and the tenant row-level policy (E-06/c01a).
- `_docs/02_document/contracts/shared_config/sc_config.md` — typed config sections and startup validation (E-01/c01).

## Problem

20 owns one table, `vaults` (data model §10.2), which carries `account_id` and must be covered by the same row-level policy as every other tenant table (ADR-014). Its root must be a node of the same account. The two vault limits come from config (helper 01): `max_vault_file_bytes` (100 MiB) and `max_vault_header_bytes`, whose default is set by E-30/c01. The epic plans to drop a former `vault_files` table, but the current data model has no such table: the wrapped file key lives in `revisions.client_meta`.

## Outcome

- One reversible migration creates `vaults` with the row-level policy and grants of the tenant tables.
- The vault config section loads with its defaults and refuses invalid values at startup.
- A schema check proves that no `vault_files` table exists.

## Scope

### Included
- `vaults` with `id`, `account_id`, `root_node_id` (unique, composite foreign key with `account_id` to `nodes`), `header` (bytes), `created_at`, `updated_at`.
- The standard tenant row-level policy and runtime-role grants on `vaults`.
- A reversible migration pair under the vault domain word (module-layout migration table: `vault`, v1.2, additive).
- A typed vault config section with `max_vault_file_bytes` (default 100 MiB = 104,857,600 bytes) and `max_vault_header_bytes` (default from E-30/c01), validated at startup.
- A schema test asserting that `vault_files` does not exist.

### Excluded
- The service logic (E-30/c03..c05).
- Any column a membership mechanism may need on 02's tables (follow-ups named by E-30/c01).
- How the migration is left out of v1.0 and v1.1 builds (flagged).

## Acceptance Criteria

**AC-1: Table shape**
Given an empty database migrated to the latest version
When the schema is inspected
Then `vaults` has the six columns, `root_node_id` is unique, `(root_node_id, account_id)` references `nodes (id, account_id)`, and no table named `vault_files` exists

**AC-2: Row-level policy (AC-59)**
Given two accounts A and B, each with one `vaults` row
When the runtime role reads `vaults` under account scope A, and inserts a row with B's `account_id` under scope A
Then the read returns 0 rows of B and the insert is refused

**AC-3: Root of the same account**
Given a folder node of account B
When a `vaults` row for account A names that node as root
Then the composite foreign key refuses the row

**AC-4: Reversible**
Given an empty database
When the migrations run up, down and up again
Then every step succeeds (sc_db_migrations up-down-up rule)

**AC-5: Config defaults and validation**
Given no vault keys in the environment
When the server config loads
Then `max_vault_file_bytes` is 104,857,600 and `max_vault_header_bytes` is the E-30/c01 default; a value that is not a positive byte size stops startup with an error that names the key

## Non-Functional Requirements

**Compatibility**
- Additive only: no existing table or column changes (data model §10.2, v1.2 additive).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Config section with no keys, valid keys, `0`, negative and non-numeric values | Defaults applied; invalid values rejected naming the key |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real PostgreSQL 18 (testcontainers), empty | Schema after migrate | Columns, unique root, composite FK, no `vault_files` | data model §10.2 |
| AC-2 | Two accounts with a vault each | Scoped read and cross-account insert | 0 rows of B; insert refused | AC-59 (sc_db_access cross-account case) |
| AC-3 | Folder of B | Insert A's vault on it | Refused by FK | IT-20-7 |
| AC-4 | Empty database | up, down, up | All succeed | sc_db_migrations up-down-up |

## Constraints

- Migrations run as `sunnycloud_migrator`; the runtime role `sunnycloud` gets only the grants of the other tenant tables (ADR-014).
- PostgreSQL 18 and sqlx 0.9 through helper 07, as for every server component.

## Risks & Mitigation

**Risk 1: The root foreign key blocks account erase**
- *Risk* (resolved by owner decision 2026-11-05): 02's erase deleted nodes before 20's hook ran, so a foreign key without a delete rule would stop 02 from deleting the vault root. The order is now 06, 20, 02, 03, 08, then 05 and 07a (20's hook runs before 02's).
- *Mitigation*: flagged below; E-30/c05 AC-4 states the required outcome; the fix (a delete rule in this migration, or 20's hook placed before 02's in 12's list by E-30/c06) is chosen with the E-13 and E-22 owners before this migration is merged.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `vaults.root_node_id` foreign key and the erase order: resolved by owner decision 2026-11-05, 20's hook runs before 02's (account erase). Still open: what happens when a user deletes or purges the vault root folder outside account erase | E-30 / E-13 / E-22 owners | open (account erase resolved) | Medium |
| How the vault migration (embedded with the server's migrations) is kept out of v1.0 and v1.1 builds is open (module-layout exception 6; E-22/c03 concern) | E-22, E-30 owners | open | Medium |
| The epic row says "drop `vault_files`", but no such table exists in the data model; the task only asserts its absence | E-30 | resolved | Low |
