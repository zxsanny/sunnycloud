# Quota crate, quota table migration and typed errors

**Task**: AZ-3749_quota_skeleton
**Name**: Quota skeleton
**Description**: The quota crate builds against the quota port declarations, ships the `quota` table with its CHECKs, grants and row-level security, converts failures into the declared `QuotaError` variants, and binds the installation default quota
**Complexity**: 2 points
**Dependencies**: AZ-3768_contracts_part1, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3652_initial_structure
**Component**: 07a quota
**Tracker**: AZ-3749
**Epic**: AZ-3631 (E-11)
**Ref**: E-11/c01
**Document dependencies**: `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` (port signatures, E-00/c03)

## Problem

The ledger, admin views and lifecycle hook all need the crate, the one table they own and the typed failures the ports declare. The runtime role must not be able to change the table's shape (ADR-014), and the ledger must never import tree or admin types (the cycle that split 07a out of 07).

## Outcome

- The quota crate depends on `contracts` and the helpers (config, error, db) only.
- A reversible migration creates `quota` (`account_id` primary key referencing `accounts`, `limit_bytes` nullable, `used_bytes` and `reserved_bytes` not null with default 0, `CHECK (used_bytes >= 0 AND reserved_bytes >= 0)`), the RLS policy and DML-only grants for `sunnycloud`.
- Database failures convert into the `QuotaError` variants declared in `contracts` (`QuotaExceeded{limit, used, reserved}`, `InvariantViolation`, `NotFound`, `InvalidLimit`) or the tier-3 unexpected error; constraint text never reaches a caller.
- The settings section binds `default_quota` (helper config table, default `100GiB`).

## Scope

### Included
- Crate dependencies, migration pair, error conversion, settings section.

### Excluded
- Ledger, admin and hook behaviour (E-11/c02 to c04).

## Acceptance Criteria

**AC-1: Migration up, down, up**
Given an empty PostgreSQL 18 database with E-06's `accounts` table
When the migration runs up, down and up as `sunnycloud_migrator`
Then `quota` exists with the columns, default values and CHECK of `data_model.md` §8.2, down removes it, and `sunnycloud` can select, insert, update and delete but cannot alter or truncate it

**AC-2: Rows are account scoped**
Given quota rows of accounts A and B
When `sunnycloud` reads under B's account scope, and then under system scope
Then B's scope sees only B's row and system scope sees both

**AC-3: Default quota setting**
Given `default_quota` unset, set to `200GiB`, and set to an unparsable value
When the settings are loaded
Then the default is 100 GiB, the set value is 200 GiB, and the bad value refuses start with a problem naming `default_quota`

**AC-4: No raw database text in errors**
Given a CHECK violation and a connection failure raised inside the crate
When they are converted
Then the first becomes a typed `QuotaError` and the second the unexpected tier, and neither message holds SQL or constraint text

## Non-Functional Requirements

**Compatibility**
- PostgreSQL 18; migration file name carries the `quota` domain word.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Settings parse | 100 GiB default; 200 GiB; bad value refused |
| AC-4 | Error conversion | Typed variant; no SQL text |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty PG 18 | Migration up, down, up; role checks | Table as specified; DML only (full ST-07a-3 in E-11/c06) | NFT-SEC-46 |
| AC-2 | Rows of A and B | Reads under B scope and system scope | Only B's row; both rows | |

## Constraints

- Crate graph: 07a depends on `contracts` and helpers only; no edge to 02, 03 or 07 (E-00 crate-graph test).
- Grants in the same migration file as the table.

## Risks & Mitigation

**Risk 1: Dependency cycle returns**
- *Risk*: a later task imports tree or admin types for convenience.
- *Mitigation*: the crate-graph test fails on any such edge.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Setting name: the helper config table says `default_quota` (`SUNNYCLOUD_DEFAULT_QUOTA`), the 07a description and E-11 say `default_quota_bytes`; this task uses the helper table name and the 07a description needs aligning | Epic owner | open | Low |
