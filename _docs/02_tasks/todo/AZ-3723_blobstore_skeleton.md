# Blobstore crate, typed errors, settings and blob tables

**Task**: AZ-3723_blobstore_skeleton
**Name**: Blobstore skeleton
**Description**: The blobstore crate builds against the port declarations, converts every failure into the declared typed errors, binds and validates its settings, and ships the migration for the `blobs` and `blob_gc` tables
**Complexity**: 3 points
**Dependencies**: AZ-3768_contracts_part1, AZ-3770_contracts_part3, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3652_initial_structure
**Component**: 04 blobstore
**Tracker**: AZ-3723
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c03

## Problem

Every blobstore task needs the crate, the conversion of I/O, database and cipher failures into the typed `BlobError` and `KeyError` the ports declare, validated settings, and the two tables it owns. No child row of the epic names the `blobs`/`blob_gc` migration: the epic's dependency line credits the tables to E-06, but `module-layout.md` (Migrations) gives the `blob` migration to 04. It is added here, so the task is 3 points instead of the planned 2.

## Outcome

- The blobstore crate builds against `contracts` part 3 and the helpers it uses (config, error, clock, ids, db, audit). No port method exists as a placeholder; later tasks add each port.
- I/O, database and cipher failures convert into the `BlobError` and `KeyError` variants declared in `contracts`; their text carries ids only.
- The blobstore settings (keys already in the helper config table) bind and validate at start.
- A reversible migration creates `blobs` and `blob_gc` as `data_model.md` §6.1 and §6.2 describe, with row-level security and DML-only grants for the API role.

## Scope

### Included
- Crate dependencies on `contracts` and helpers; no edge to any other component crate.
- Conversions into the declared error variants; `Display` with ids only, `Debug` of key-bearing values redacted.
- Settings section: `blob_root`, `retention` (0 to 90 days in release builds), `orphan_*`, `blob_free_reserve`, `gc_*`, `scrub_*`, `blob_sync_coalesce_delay`, `blob_preallocate`, `blob_read_ahead_chunks`, `blob_io_threads`, `master_key_file`, `master_key_ring`, `master_key_current`.
- Migration pair: `blobs` (states `uploading`, `committed`, `gc_pending`; `unavailable_at`, `last_scrubbed_at`; indexes `UNIQUE (id, account_id)`, `(account_id)`, scrub index `(last_scrubbed_at NULLS FIRST, id) WHERE state = 'committed'`) and `blob_gc` (`blob_id`, `account_id`, `due_at`, `reason`; index `(due_at)`), RLS policy, grants.

### Excluded
- Port behaviour (E-10/c04 to c13), metrics (E-10/c15).
- The error enums themselves: they are declared in `contracts` by E-00/c05; this task adds no second definition.

## Acceptance Criteria

**AC-1: Retention range enforced at start**
Given a release build and `retention` set to 91 days, then to 0 and to 90 days
When the settings are loaded
Then 91 days refuses start with a problem naming `retention`, and 0 and 90 days are accepted

**AC-2: Migration up, down, up**
Given an empty PostgreSQL 18 database
When the migration runs up, down and up as `sunnycloud_migrator`
Then both tables exist with the columns, constraints and indexes of `data_model.md` §6.1 and §6.2, down removes them, and the API role `sunnycloud` can select, insert, update and delete but cannot alter the tables

**AC-3: Rows are account scoped**
Given `blobs` rows of accounts A and B
When the API role reads under B's account scope, and then under system scope
Then B's scope sees only B's rows and system scope sees all rows

**AC-4: Typed errors carry ids only**
Given each error variant produced from an I/O, database and cipher failure
When it is formatted with `Display` and `Debug`
Then the text holds blob and account ids only and no key bytes, and no conversion drops an error (the silent-catch lint passes)

## Non-Functional Requirements

**Compatibility**
- PostgreSQL 18; migration file name carries the `blob` domain word so the owner glob matches.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Settings validation with 91 d, 90 d, 0 d | 91 d refused naming the key; others accepted |
| AC-4 | Formatting of every error variant built from a failure | Ids only; no key bytes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Release-build settings | Start with retention 91 d | Start refused naming `retention` | FT-P-17 |
| AC-2 | Empty PG 18 | Migration up, down, up | Tables as specified; API role has DML only | NFT-SEC-46 |
| AC-3 | Rows of A and B | Reads under B scope and system scope | Only B's rows; all rows (full suite ST-04-6 in E-10/c14) | |

## Constraints

- One migration pair per owner domain word (`module-layout.md` Migrations); grants in the same file as the tables.
- Settings come from the helper config table; no new key and no second spelling.

## Risks & Mitigation

**Risk 1: Foreign keys order**
- *Risk*: `blobs` references `accounts` and must sort after E-06's minimal `accounts` migration; 02's `revisions` references `blobs`.
- *Mitigation*: depend on E-06/c01; the migration timestamp sorts after `accounts_base` and before 02's tables.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Epic dependency line credits the `blobs`/`blob_gc` tables to E-06, `module-layout.md` gives them to 04; this task owns them | Decompose | resolved | Low |
| `BlobError`/`KeyError` are declared in `contracts` (E-00/c05); if E-00/c05 leaves them out, they are added there by a `contracts` change task, not in this crate | E-00 | open | Low |
