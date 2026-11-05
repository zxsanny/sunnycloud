# Identity crate skeleton, identity migration, config section and test harness

**Task**: AZ-3682_identity_crate_migration_config
**Name**: Identity skeleton, schema and config
**Description**: Give the identity crate its schema (identity columns on `accounts` plus seven identity tables), its typed config section with start validation, and the component test harness that every later identity task uses
**Complexity**: 3 points
**Dependencies**: AZ-3768_contracts_part1, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3652_initial_structure; notes: E-31/c13 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 01 identity
**Tracker**: AZ-3682
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c02

## Problem

Every identity service reads and writes eight tables. E-06 creates a minimal `accounts` table (id, state, created_at) before any tenant table (owner decision 2026-11-04); identity must extend it, not create it. The services also need typed settings with safe defaults and a start check, and a shared test setup on real PostgreSQL 18 with the mock identity provider.

## Outcome

- One reversible migration pair (domain word `identity`, the `create_identity_tables` baseline of data model §13.1) adds the identity columns of `accounts` and creates `identities`, `sessions`, `device_tokens`, `device_grants`, `invites`, `oauth_states` and `login_codes` with the keys, constraints, indexes, grants and row-level security of data model §4 and §12.3.
- A typed identity config section with every key of 01 §6, its defaults and its start validation.
- A component test harness: per-test database from a migrated template, the component connected as role `sunnycloud`, fixtures and probes as `sunnycloud_migrator`, the manual clock, mock-idp reachable as configured providers.
- The identity crate builds with its error types and config section; it implements no port yet.

## Scope

### Included
- `accounts`: add `display_name`, `role` (`user` or `admin`), `deletion_requested_at`, and the index `accounts_state_idx (state) WHERE state <> 'active'`. Columns created by E-06 are not renamed or retyped.
- The seven identity tables as data model §4.2 to §4.8 define them, including: `UNIQUE (provider, provider_user_id)`; UNIQUE hash columns; `UNIQUE (user_code) WHERE status = 'pending'`; one open setup token (`UNIQUE ((true)) WHERE kind = 'setup' AND redeemed_at IS NULL`); the `invites` CHECKs on `kind`, `grants_role`, `target_account_id`, `created_by`; the `return_to` CHECK on `oauth_states`; nullable `code_challenge` on `oauth_states`.
- Grants to `sunnycloud` (DML only) in the same file; row-level security enabled (no `FORCE`) on the account-scoped tables `identities`, `sessions`, `device_tokens`, `device_grants`.
- Config keys of 01 §6 with defaults: `access_token_ttl` 15 min, `refresh_token_ttl` 30 d, `refresh_reuse_grace` 30 s, `session_cache_ttl` 2 s, `session_last_seen_interval` 1 min, `device_code_ttl` 10 min, `device_poll_interval` 5 s, `oauth_state_ttl` 10 min, `login_code_ttl` 2 min, `invite_ttl` 7 d, `setup_token_ttl` 7 d, `provider_timeout` 10 s, the AC-20 limit numbers, `public_url`, `providers[]`, `email_allowlist[]`, `geoip_db_path`, `allow_plain_http_dev` (off). Provider secrets come from files.
- Start validation of the section (run by E-22/c04 at start).
- Test harness parts listed in Outcome.

### Excluded
- The minimal `accounts` table and the `Migrator` (E-06/c01); the config loader and key table (E-01/c01); binding the whole config at start (E-22/c04).
- Port implementations (E-15/c03 onward).
- The mock-idp service itself (E-31/c13) and the GeoIP fixture file (E-31/c22).

## Acceptance Criteria

**AC-1: Migration applies, reverts and re-applies**
Given an empty PostgreSQL 18 database with E-06's base migration applied
When the identity migration runs up, down, up as `sunnycloud_migrator`
Then each step succeeds, and after the last up the seven tables and the new `accounts` columns and index exist

**AC-2: Data-model constraints hold**
Given the migrated schema
When fixture inserts break a rule
Then the insert fails for: a duplicate `(provider, provider_user_id)`; a duplicate token, refresh or device-code hash; a second pending grant with the same `user_code`; a second open setup token; a `return_to` that does not start with `/` or starts with `//`; a setup row whose `grants_role` is not admin or that has a `target_account_id` or a `created_by`

**AC-3: Runtime role and row-level security**
Given fixture rows for accounts A and B
When the component's `sunnycloud` connection reads `identities`, `sessions`, `device_tokens` and `device_grants` scoped to B, reads them with no scope set, and tries `CREATE TABLE`
Then it sees only B's rows, sees no rows without a scope, and the DDL fails with permission denied

**AC-4: Config defaults**
Given an identity config section with only the required keys (`public_url`, one provider)
When it is loaded
Then every other key has the default listed in Scope

**AC-5: Start validation refuses unsafe settings**
Given one config per case: no provider; a provider kind other than `github` or `oidc` (for example `facebook`, FT-N-28 step 2); `public_url` relative or `http://` with `allow_plain_http_dev` off; `device_code_ttl` above 10 min; `session_cache_ttl` above 3 s
When the section is validated
Then each is refused with a message that names the key, and the same config with the value fixed is accepted

**AC-6: Test harness**
Given the harness
When two tests run in parallel and finish
Then each had its own database cloned from the migrated template, the component used role `sunnycloud`, fixtures used `sunnycloud_migrator`, time came from the manual clock, providers pointed at mock-idp, and the connection count returned to its baseline

## Non-Functional Requirements

**Reliability**
- `session_cache_ttl` above 3 s breaks AC-21 (E-15 risk 2); the ceiling in AC-5 is the guard.

**Compatibility**
- Migration is a reversible pair; up, down, up passes in CI (data model §13.1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Defaults of the identity section | Each default equals the value in Scope (read, not waited) |
| AC-5 | Validation per refusal case | Refused with the key named; fixed value accepted |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty DB plus E-06 base migration | Up, down, up | All succeed; tables, columns, index present | |
| AC-2 | Migrated schema | Violating fixture inserts | Each insert fails | |
| AC-3 | Rows for A and B | Scoped, unscoped reads; DDL as `sunnycloud` | Only B rows; none unscoped; DDL denied | ST-01-2, NFT-SEC-46 |
| AC-5 | One config per case | Section validation | Each refused naming the key | FT-N-25, FT-N-28 |
| AC-6 | Harness | Two parallel tests | Separate DBs, roles, manual clock, connections back to baseline | |

## Constraints

- Migration ownership follows the `identity` domain word of `module-layout.md`; grants to `sunnycloud` go in the same file.
- No rename or retype of E-06's `accounts` columns without owner confirmation.
- Test isolation: one database per test from a migrated template; transactions are not the isolation device (01 `tests.md`, Test Data Management).

## Risks & Mitigation

**Risk 1: Ordering against E-06's base migration**
- *Risk*: the identity migration runs before the minimal `accounts` table exists.
- *Mitigation*: the identity migration's timestamp sorts after E-06's; AC-1 starts from E-06's migration.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Role value naming: 01's description says `member`, the data model says `user` (`accounts.role`, `invites.grants_role` default). This task follows the confirmed data model (`user`); 01's text should be aligned | E-15 owner | open | Low |
| Who owns the CHECK and default of `accounts.state` (E-06 minimal table or this migration) is not stated | E-06 / E-15 owners | open | Low |
| E-22/c04 also lists provider validation; the rule lives once, in this section's validation, and E-22/c04 calls it | E-22 owner | open | Low |
| A "Google preset" appears in research notes only; 01's description configures Google as a generic OIDC provider, so no preset is built here | E-15 owner | open | Low |
