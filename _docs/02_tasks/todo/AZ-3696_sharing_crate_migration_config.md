# Sharing crate skeleton, share_links migration, config section and grant-key loading

**Task**: AZ-3696_sharing_crate_migration_config
**Name**: Sharing skeleton, schema and config
**Description**: Give the sharing crate its `share_links` table, its typed config section with defaults, and the start-time refusal when the share grant key is missing
**Complexity**: 2 points
**Dependencies**: AZ-3695_sharing_gaps_grant_key_spike, AZ-3770_contracts_part3, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3706_tree_skeleton, AZ-3652_initial_structure
**Component**: 06 sharing
**Tracker**: AZ-3696
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c02

## Problem

Share links need one table tied to the tree's nodes so a purged file removes its links. The services need typed settings (lifetimes, cache, limits, password cost) and a secret key for access grants; without that key the server must not start.

## Outcome

- One reversible migration pair (domain word `share`) creates `share_links` per data model §8.1 with grants and row-level security.
- A typed sharing config section with the defaults of 06 §6 and start validation.
- The grant key is loaded as a secret through the entry point decided in E-16/c01; start is refused without it.
- A component test harness like identity's: per-test database from the migrated template, role `sunnycloud` for the component, migrator for fixtures, manual clock, a test grant key, a fresh `AbuseLimiter` per test.

## Scope

### Included
- `share_links`: `id`, `account_id`, `node_id` (composite FK to `nodes`, `ON DELETE CASCADE`), `token_hash` UNIQUE, `password_hash` NULL, `expires_at` NOT NULL, `revoked_at` NULL, `created_at`; indexes `(account_id)`, `(node_id)`; DML grants to `sunnycloud`; row-level security enabled (no `FORCE`).
- Config keys with defaults: `share_default_ttl` 7 d, `share_max_ttl` 365 d, `share_lookup_cache` 1 s, `share_grant_ttl` 10 min, `share_wrong_password_limit` 10 per `share_wrong_password_window` 10 min, `share_invalid_token_limit` 100 per minute per IP, `share_password_max_concurrency`, Argon2id parameters, `share_grant_key` (secret).
- Start refusal for a missing or unusable grant key, at the entry point named by E-16/c01.

### Excluded
- The `nodes` table (E-13/c02); the config loader and secret files (E-01/c01); the start sequence (E-22/c05).
- Service logic (E-16/c03 onward).

## Acceptance Criteria

**AC-1: Migration applies, reverts and re-applies**
Given a database with the tree migrations applied
When the share migration runs up, down, up as `sunnycloud_migrator`
Then each step succeeds and `share_links` exists with its columns, unique `token_hash`, two indexes and the composite FK

**AC-2: A purged node removes its links**
Given a link row on a node (fixtures)
When the node row is deleted (fixture delete as the migrator role; the real purge path is tested in E-16/c06)
Then the link row is gone through `ON DELETE CASCADE`

**AC-3: Runtime role and row-level security**
Given link rows for accounts A and B
When the `sunnycloud` connection selects `share_links` scoped to B, with no scope, and tries DDL
Then it sees only B's rows, no rows without a scope, and DDL is denied

**AC-4: Config defaults**
Given a sharing section with only the grant key set
When it is loaded
Then every other key has the default listed in Scope

**AC-5: No grant key, no start (IT-06-12)**
Given a configuration without `share_grant_key`, and one with an unusable key as defined by E-16/c01
When the entry point named by E-16/c01 runs
Then it refuses with a message that names `share_grant_key`, and with a valid key it succeeds

## Non-Functional Requirements

**Compatibility**
- Reversible pair; up, down, up passes in CI (data model §13.1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Defaults of the sharing section | Each default equals 06 §6 (read, not waited) |
| AC-5 | Key validation cases | Missing and unusable refused naming the key |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Tree migrations applied | Up, down, up | Table, unique hash, indexes, FK present | |
| AC-2 | Link on a node | Node row deleted | Link row cascaded | |
| AC-3 | Rows for A and B | Scoped, unscoped reads; DDL | Only B; none; denied | ST-06-8, NFT-SEC-46 |
| AC-5 | Missing and unusable key | IT-06-12 entry point | Refused naming the key; valid key ok | |

## Constraints

- Migration file owned by the `share` domain word (`module-layout.md`); grants in the same file.
- The grant key never appears in logs or config dumps (E-01 `Secret`).

## Risks & Mitigation

**Risk 1: FK ordering against the tree migration**
- *Risk*: the share migration runs before `nodes` exists.
- *Mitigation*: timestamp after E-13/c02's migration; AC-1 starts from the tree migrations.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No default is stated for `share_password_max_concurrency` or for the Argon2id memory, iterations and parallelism; PT-06-3 and ST-06-5 depend on them | E-16 owner (architect) | open | Medium |
