# Tree crate skeleton, errors, config and tables

**Task**: AZ-3706_tree_skeleton
**Name**: Tree skeleton
**Description**: Wire the tree crate to its helpers and ports, define its typed errors and config section, and create the `nodes` and `revisions` tables with their indexes, constraints, grants and row-level security
**Complexity**: 3 points
**Dependencies**: AZ-3769_contracts_part2, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3786_sc_observability, AZ-3723_blobstore_skeleton, AZ-3652_initial_structure
**Component**: 02_tree
**Tracker**: AZ-3706
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.

## Problem

Every tree task needs the same base: the two owned tables with the unique name index that arbitrates collisions, typed errors with stable problem codes, the config keys, and one rule for retrying a transaction the database aborted. Without one base task, each later task would add pieces of the schema and error set on its own.

## Outcome

- `nodes` (all columns except `seq`) and `revisions` exist as `data_model.md` §5 describes, with grants for the runtime role and row-level security on.
- The tree's typed errors map to the stable codes and statuses of helper 02.
- The tree config keys load with their defaults and accept millisecond overrides in tests.
- One retry-once rule for deadlock or serialization aborts is available to every tree operation.

## Scope

### Included
- Migrations for `nodes` and `revisions`: composite FKs with `account_id`, `UNIQUE(id, account_id)`, `nodes_parent_name_key` (unique on parent and `name_key` for live rows), `nodes_root_key`, `nodes_parent_idx`, `nodes_tombstone_idx`, `nodes_cascade_idx`, `revisions.blob_id` UNIQUE with FK `RESTRICT` to `blobs`, `nodes.current_revision_id` FK deferred with `ON DELETE SET NULL`, `(node_id, created_at)`, `(account_id, content_hash)`, `revisions_client_device_idx`, the name, size, blocklist and `client_meta` CHECKs, grants (DML only) and RLS policies.
- Typed `TreeError` with codes `name_conflict` 409, `invalid_name` 422, `cycle` 409, `edited_since` 409, `parent_deleted` 410, `parent_not_found` 404, `not_a_folder` 422, `invalid_cursor` 422, `batch_too_large` 422, `not_found` 404.
- Config section: `tombstone_horizon` (90 d), `listing_default_limit` (1000), `listing_max_limit`, `cascade_batch_size` (5000), `purge_batch_size` (5000), `tree_maintenance_interval` (30 s), `conflict_copy_time_zone` (UTC).
- Use of the account advisory lock from helper 07 and the retry-once rule for aborted transactions.

### Excluded
- `nodes.seq` and `nodes_feed_idx` (05's migration, E-12/c02).
- Any tree operation (later E-13 tasks).

## Acceptance Criteria

**AC-1: Tables apply and revert**
Given an empty database with the minimal `accounts` migration (E-06) and the `blobs` table (E-10)
When the migrations run up, down and up again as `sunnycloud_migrator`
Then `nodes` and `revisions` exist with every index and constraint listed in Scope, RLS is enabled, and role `sunnycloud` can select, insert, update and delete but not alter the tables

**AC-2: Unique index arbitrates names**
Given a live child `Docs` under a folder
When a second live child with the same `name_key` is inserted
Then the database raises a unique violation, while a tombstoned child with that `name_key` does not block a new live one

**AC-3: Revision foreign keys**
Given a file node whose current revision points at a blob
When that blob row is deleted while the revision exists, and separately the revision row is deleted
Then the first is refused (RESTRICT) and the second clears `nodes.current_revision_id`

**AC-4: Error codes**
Given each `TreeError` variant
When it is converted through helper 02
Then it yields exactly the code and status listed in Scope

**AC-5: Config defaults and overrides**
Given no tree keys set, and separately `tombstone_horizon` set to 2 s
When the config loads
Then the defaults listed in Scope apply, and the override yields 2 s

**AC-6: Retry once on an aborted transaction**
Given a tree operation whose transaction the database aborts for deadlock or serialization
When the abort happens once, and in a second run twice
Then the first run succeeds on its single retry, and the second fails as an unexpected error after exactly one retry

## Non-Functional Requirements

**Reliability**
- Applied migration files are immutable; changes are new files (E-00 migration rules).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Variant-to-code table | Every variant maps to its code and status |
| AC-5 | Defaults and overrides | Listed defaults; ms override honoured |
| AC-6 | Retry wrapper with injected aborts | One retry, then unexpected error |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty PostgreSQL 18 | Migrate up, down, up; grant probe as `sunnycloud` | Schema as specified; DDL denied | NFT-SEC-46 |
| AC-2 | Folder with live `Docs` | Insert same `name_key` live, then after tombstone | Unique violation; tombstone does not block | AC-42 |
| AC-3 | File with current revision | Delete blob row; delete revision row | RESTRICT; pointer cleared | |

## Constraints

- Migrations belong to component 02 and use the `tree` domain word (module-layout § Migrations).
- Port signatures come from `contracts` (E-00/c04); a signature change is a separate `contracts` task (Layout Rule 6).

## Risks & Mitigation

**Risk 1: Migration order with 05 and 04**
- *Risk*: 05 adds `seq` to `nodes` and `revisions` references `blobs`, so order matters.
- *Mitigation*: AC-1 runs on top of the E-06 and E-10 migrations; E-12/c02 runs after this task.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `listing_max_limit` has no default value in 02 §6 | E-13 owner / E-01 | open | Low |
| G-1 (stored normalisation form) may change how `name_key` is computed (E-13/c01) | Owner | open | Low |
