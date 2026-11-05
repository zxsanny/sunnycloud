# sc_db migrator, schema version check, health and stats, and the first `accounts` migration

**Task**: AZ-3783_sc_db_migrator_accounts_base
**Name**: sc_db `Migrator`, `check_version`, health, stats and the `accounts` base migration
**Description**: Apply, revert and report sqlx 0.9 reversible migrations embedded from the migrations folder, tell `Current`, `Pending` and `UnknownNewer` apart, expose database health and a stats snapshot, and ship the first migration: a minimal `accounts` table every tenant table can reference
**Complexity**: 3 points
**Dependencies**: AZ-3782_sc_db_access, AZ-3740_migrations_setup, AZ-3741_docker_compose_skeletons
**Component**: shared/db (helper 07)
**Tracker**: AZ-3783
**Epic**: AZ-3638 (E-06)
**Ref**: E-06/c01b

## Document Dependencies

- `_docs/02_document/contracts/shared_db/sc_db_access.md` — `Db`, `DbRole`, `DbError`, produced by E-06/c01a.

## Problem

Upgrades must roll back cleanly and the API must refuse a schema it does not match (RESTRICT-32, AC-65). Tenant tables of several epics (blobs, quota, feed, tree) reference `accounts` with foreign keys, but the identity epic that owns accounts comes later (E-15). Owner decision 2026-11-04: E-06 writes a minimal `accounts` migration (id, state, created_at) before any tenant table, and E-15 extends it.

## Outcome

- The one-shot migrate step can apply and revert migrations as the migrator role; the API role cannot.
- `check_version` reports `Current`, `Pending{n}` or `UnknownNewer`, so `serve`, `/readyz` and `doctor` share one answer.
- `accounts (id, state, created_at)` exists from the first migration, with DML grants to the API role, so later tenant tables can reference it.
- Health and a read-only stats snapshot feed readiness and the E-09 collectors.

## Scope

### Included
- `Migrator::run_pending`, `revert`, `status`, `check_version` over the reversible pairs embedded from the migrations folder, each migration in one transaction under sqlx's advisory lock; `MigrationFailed`, `ChecksumMismatch`, `UnknownNewer`, `IrreversibleMigration`.
- The first migration pair (domain word `accounts_base`): table `accounts` with `id uuid` primary key (no default), `state text NOT NULL` with the named check `accounts_state_check` (`active`, `disabled`, `deleting`), `created_at timestamptz NOT NULL` (no default); `SELECT, INSERT, UPDATE, DELETE` granted to `sunnycloud` in the same file; `SELECT` on the migrations bookkeeping table granted to `sunnycloud`; a `down` that drops what the `up` created. No row-level security on `accounts`.
- `Db::health` (`reachable`, `server_version`, `migrations_pending`, `pool_in_use`) and `Db::stats` (pool state, query durations by `op`, pending migrations).
- Migration authoring rules in the crate README (reversible pairs, owner header for data rewrites, `-- no-transaction` idempotence).
- Integration tests on a real PostgreSQL 18 with both roles.

### Excluded
- The identity columns of `accounts` (`display_name`, `role`, `deletion_requested_at`) and `accounts_state_idx` (E-15).
- Every other table (owning epics); the audit table (E-07).
- The `migrate` and `serve` commands and readiness registration (E-22, E-21); the previous-release snapshot run in CI (needs a release).

## Acceptance Criteria

**AC-1: First migration creates the base table (owner decision 2026-11-04)**
Given an empty PostgreSQL 18 with both roles
When `run_pending` runs as the migrator
Then `accounts` exists with exactly the columns `id`, `state`, `created_at`, owned by `sunnycloud_migrator`, `sunnycloud` holds `SELECT, INSERT, UPDATE, DELETE` and owns nothing, and an insert with `state = 'archived'` fails with `CheckViolation`

**AC-2: Tenant tables can reference it**
Given the migrated schema and a test table with `account_id` referencing `accounts (id) ON DELETE RESTRICT`
When rows are inserted for an existing and for a missing account, and the referenced account is deleted
Then the first insert succeeds, the second fails with `ForeignKeyViolation`, and the delete fails with `ForeignKeyViolation`

**AC-3: Up, down, up (E-00 AC 6)**
Given an empty database
When the migrator applies all migrations, reverts all and applies all again
Then every step succeeds and the final schema equals the first

**AC-4: Schema states (E-06 AC 6)**
Given a fully migrated database, then a binary with one more migration file, then a database with an applied version the binary does not know
When `check_version` runs as `sunnycloud` each time
Then it returns `Current`, `Pending{n: 1}` and `UnknownNewer`

**AC-5: Interrupted migration (AC-65, E-06 AC 3)**
Given a seeded long-running migration after the base migration
When the migrating process is killed during it
Then the base migration stays applied, the killed one leaves no change, `check_version` returns `Pending`, and the next `run_pending` applies it (FT-N-39 component part)

**AC-6: Applied files are immutable**
Given an applied migration whose file content was changed afterwards
When `run_pending` runs
Then it returns `ChecksumMismatch` and applies nothing

**AC-7: Irreversible down is reported**
Given a seeded migration whose `down` raises `irreversible: restore from backup`
When `revert` reaches it
Then it returns `IrreversibleMigration` and the schema is unchanged

**AC-8: The API role cannot migrate (ADR-014)**
Given a pool connected as `sunnycloud`
When `run_pending` is attempted
Then it fails without changing the schema, while `check_version` and `status` succeed

**AC-9: Health and stats**
Given a running pool with one pending migration, and then a stopped database
When `Db::health` and `Db::stats` are read
Then the first reading shows `reachable`, the server version, `migrations_pending = 1` and the pool in use, and the second shows `reachable = false` without blocking longer than the acquire timeout

## Non-Functional Requirements

**Reliability**
- Each migration is atomic; a failure exits the migrate step non-zero and leaves the earlier ones applied.

**Security**
- Grants to `sunnycloud` come only from migrations; the API role owns no object.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Version comparison logic for current, pending and unknown-newer sets | Correct `SchemaState` |
| AC-6 | Checksum comparison | Mismatch detected |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty PG 18 | First migration (integration) | Table, owner, grants, check | Security |
| AC-3 | Empty PG 18 | Up, down, up (integration; also `pg18-migrations` in CI) | All steps succeed | — |
| AC-5 | Seeded slow migration | Kill mid-way; later FT-N-39 and NFT-RES-14 through E-22 | Rolled back; `Pending`; re-run applies | Reliability |
| AC-8 | API role | `run_pending` as `sunnycloud` | Refused | Security |

## Constraints

- sqlx 0.9 reversible pairs `YYYYMMDDHHMMSS_<verb>_<object>`; the domain word of this pair is `accounts_base` (module-layout § Migrations).
- No SQL `now()` and no `DEFAULT now()`; ids and times come from the application (data_model §1).
- No table, column or database rename without owner confirmation.

## Risks & Mitigation

**Risk 1: sqlx dirty state after an interruption**
- *Risk*: sqlx 0.9 might not block start after a killed `-- no-transaction` migration (data_model §13.1 "Verify at sqlx 0.9").
- *Mitigation*: AC-5 tests the transactional case; the `-- no-transaction` case is verified against sqlx 0.9 in this task and reported if unsupported.

## Runtime Completeness

- Named capability: sqlx 0.9 migrations on PostgreSQL 18 with the two real roles.
- Production code that must exist: the embedded migrator used by the `migrate` step and the version check used by `serve`.
- Allowed external stubs: none.
- Unacceptable substitutes: running migrations as the API role, skipping the version check, a schema created by test setup code instead of the migration files.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Split row of E-06 (see E-06/c01a). This part carries the owner-decided `accounts` migration. | Decompose parent | open | Low |
| E-15 will add `display_name` and `role` as `NOT NULL` (data_model §4.1); adding NOT NULL columns without a default is safe only while no release exists. | E-15 owner | open | Low |
| E-07 calls `create_audit_log` the first real migration; the `accounts_base` pair now comes first. | E-07 owner | resolved | Low |
| Whether sqlx 0.9 blocks start after an interrupted `-- no-transaction` migration is unverified (data_model §13.1). | E-06 owner | open | Medium |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_db/sc_db_migrations.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
