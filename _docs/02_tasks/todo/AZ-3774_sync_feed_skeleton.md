# Sync feed crate, feed table, `nodes.seq` and settings

**Task**: AZ-3774_sync_feed_skeleton
**Name**: Sync feed skeleton
**Description**: The sync-feed crate builds against the feed port declarations, ships the `account_feeds` table and the `nodes.seq` column with `nodes_feed_idx` on 02's table, converts failures into the declared errors, and binds and validates the long-poll settings
**Complexity**: 2 points
**Dependencies**: AZ-3769_contracts_part2, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3706_tree_skeleton, AZ-3652_initial_structure
**Component**: 05 sync_feed
**Tracker**: AZ-3774
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c02
**Document dependencies**: `_docs/02_document/contracts/05_sync_feed/feed-writer-ports.md` (the `nodes.seq` ownership rule this task creates)

## Problem

Every feed task needs the crate, the per-account counter row and the stamp column. `nodes.seq` belongs to 05 but sits in 02's `nodes` table (`module-layout.md`, Migrations: the `feed` migration adds only `nodes.seq` and `nodes_feed_idx` to 02's table), so this migration must run after 02's `nodes` migration and touch nothing else of that table.

## Outcome

- The sync-feed crate depends on `contracts` and the helpers (config, error, clock, db) only.
- A reversible migration creates `account_feeds` (`account_id` primary key referencing `accounts`, `epoch` not null, `seq` and `purge_horizon` not null with default 0), its RLS policy and DML-only grants, and adds `nodes.seq` (bigint, not null, default 0) and `nodes_feed_idx (account_id, seq)` to 02's `nodes` table.
- Failures convert into the error variants declared in `contracts` for the feed ports (`AccountFeedMissing`, `ResyncRequired{reason}`, `BadCursor`, `Unauthenticated`) or the tier-3 unexpected error.
- Settings bind and validate: `longpoll_wait_max` (30 s; 1 s to 55 s), `longpoll_auth_recheck` (2 s), `feed_safety_recheck` (10 s), `feed_page_limit_default` (500), `feed_page_limit_max`, `max_holds_per_principal` (4).

## Scope

### Included
- Crate dependencies, migration pair, error conversion, settings section.

### Excluded
- Any feed behaviour (E-12/c03 to c07); the `nodes` table itself (E-13/c02).

## Acceptance Criteria

**AC-1: Migration up, down, up after 02's table**
Given an empty PostgreSQL 18 database with E-06's `accounts` and 02's `nodes` migrations applied
When the feed migration runs up, down and up as `sunnycloud_migrator`
Then `account_feeds` exists as `data_model.md` §7.1 describes, `nodes` has `seq` (default 0) and `nodes_feed_idx`, down removes exactly these and leaves every other `nodes` column and index unchanged, and the migration sorts after 02's `nodes` migration

**AC-2: Roles and row scope**
Given feed rows of accounts A and B
When `sunnycloud` reads under B's account scope, tries `ALTER TABLE account_feeds`, and reads under system scope
Then B's scope sees only B's row, the DDL is refused, and system scope sees both rows

**AC-3: Wait range enforced at start**
Given `longpoll_wait_max` set to 500 ms, 56 s, 1 s and 55 s
When the settings are loaded
Then 500 ms and 56 s refuse start with a problem naming `longpoll_wait_max`, and 1 s and 55 s are accepted; unset keys take the defaults listed above

**AC-4: Typed errors without SQL text**
Given a missing feed row and a database failure inside the crate
When they are converted
Then the first becomes `AccountFeedMissing` and the second the unexpected tier, with no SQL text in either message

## Non-Functional Requirements

**Compatibility**
- PostgreSQL 18; migration file name carries the `feed` domain word.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Settings validation | Out-of-range wait refused; defaults applied |
| AC-4 | Error conversion | Typed variant; no SQL text |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | PG 18 with `accounts` and `nodes` | Migration up, down, up | Feed table and column as specified; other `nodes` columns untouched | |
| AC-2 | Rows of A and B | Scope reads and DDL attempt | Only B's row; DDL refused (full ST-05-2 in E-12/c08) | NFT-SEC-46 |

## Constraints

- Crate graph: 05 depends on `contracts` and helpers only; it never calls 02.
- `nodes.seq` is written only by the stamp (E-12/c03); 02 never writes it (`feed-writer-ports.md`).

## Risks & Mitigation

**Risk 1: Migration order across owners**
- *Risk*: the feed migration sorts before 02's `nodes` migration and fails on a fresh install.
- *Mitigation*: AC-1 runs on a database where 02's migration is applied first; the timestamp is chosen after it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `longpoll_auth_recheck` "must stay below 5 s minus the session cache TTL" (01's setting); the spec does not say whether start-up validates this cross-component rule | Epic owner / E-01 | open | Low |
| `feed_page_limit_max` has no default value in the helper config table | Epic owner / E-01 | open | Low |
