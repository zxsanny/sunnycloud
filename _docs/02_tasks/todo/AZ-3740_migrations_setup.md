# Migrations setup: directory, offline query data, prepare check and migration check

**Task**: AZ-3740_migrations_setup
**Name**: Migrations setup
**Description**: Create the empty migrations area and the committed sqlx offline data, plus the two repository checks the CI jobs call: offline data up to date, and up, down, up of every migration on an empty PostgreSQL 18
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: Bootstrap (migrations)
**Tracker**: AZ-3740
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c10

## Problem

Every owning epic will add reversible migration pairs, and compile-time checked queries need committed offline data (`SQLX_OFFLINE`), but nothing yet holds the files or proves they stay consistent. The CI jobs `rust-lint` and `pg18-migrations` (E-00/c09) must call repository scripts, not inline YAML (ci_cd_pipeline § Principles), so those checks have to exist first.

## Outcome

- An empty migrations area and an empty committed offline-query area exist where sqlx and the later `sc_db` migrator (E-06) expect them.
- One check fails when the committed offline query data does not match the code.
- One check applies every migration to an empty PostgreSQL 18, reverts them all and applies them again, and passes with zero migrations.

## Scope

### Included
- The migrations area (empty) and the committed offline query data area (empty), with the workspace configured so `SQLX_OFFLINE=true` builds work.
- The pinned sqlx command-line tool version matching sqlx 0.9, and how a developer installs it.
- The offline-data check (`cargo sqlx prepare --check --workspace`) as a repository script that takes the database URL from the environment.
- The migration check as a repository script: against a given empty PostgreSQL 18, apply all, revert all, apply all, printing each step.
- Developer instructions for both checks (in the crate-independent part of the README section that E-00/c12 writes, or beside the scripts).

### Excluded
- Wiring the two scripts into CI jobs and choosing which job runs the prepare check (E-00/c09; the job needs the PG 18 service).
- The `Migrator` that embeds the files in the binary, the first migration and the interrupted-migration test (E-06).
- Any table (owning epics).

## Acceptance Criteria

**AC-1: Empty setup passes both checks**
Given a clean checkout and an empty PostgreSQL 18 reachable through the database URL
When the offline-data check and the migration check run
Then both exit 0, and the migration check reports 0 migrations applied, reverted and re-applied

**AC-2: Up, down, up runs every pair**
Given a test branch with two seeded reversible migration pairs
When the migration check runs against an empty PostgreSQL 18
Then it applies both, reverts both, applies both again and exits 0; a seeded `down` file with an SQL error makes it exit non-zero and name the failing version (E-00 AC 6)

**AC-3: Stale offline data fails**
Given a seeded compile-time checked query added without regenerating the offline data
When the offline-data check runs
Then it exits non-zero

**AC-4: Verbose output**
Given either check
When it runs
Then every step prints a line, and neither script uses a quiet flag

## Non-Functional Requirements

**Performance**
- Each check finishes in under 1 minute with zero migrations.

**Compatibility**
- PostgreSQL 18 (digest-pinned image in CI), sqlx 0.9 and the matching command-line tool.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Search of the two scripts for quiet flags | 0 hits |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty PG 18, 0 migrations | Both checks | Exit 0; 0 applied | Performance |
| AC-2 | Two seeded pairs; then a broken `down` | Migration check | Up, down, up succeeds; broken `down` exits non-zero naming the version | — |
| AC-3 | Seeded query, stale offline data | Offline-data check | Exit non-zero | — |

## Constraints

- Migration files are reversible pairs `YYYYMMDDHHMMSS_<verb>_<object>` (data_model §13.1; module-layout § Migrations); sqlx does not mix reversible and simple files.
- CI YAML only calls repository scripts (ci_cd_pipeline § Principles).

## Risks & Mitigation

**Risk 1: Two checks of migration state drift apart**
- *Risk*: the CI script (command-line tool) and the product `Migrator` (E-06) apply files differently.
- *Mitigation*: both read the same files with sqlx 0.9; E-06's tests run up, down, up through the `Migrator` too.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The previous-release schema snapshot and the mid-way kill of `pg18-migrations` (ci_cd_pipeline PR gate) need a release and the `Migrator`; AZ-3652 assigns them to E-06, not to this task. | E-06 owner | accepted-risk | Low |

## Contract

This task has no new contract file; its consumers (E-00/c09, E-06/c01b) rely on the migration rules of `_docs/02_document/data_model.md` §13.1 and `_docs/02_document/module-layout.md` § Migrations, and on `_docs/02_document/contracts/shared_db/sc_db_migrations.md` once E-06 lands.
