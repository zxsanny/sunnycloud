# Contract: sc_db migrations — migrator, schema version, health and stats, the `accounts` base table

**Component**: shared/db (crate `sc_db`; migrations under `src/migrations/`)
**Producer task**: E-06/c01b — `_docs/02_tasks/todo/B01-17_sc_db_migrator_accounts_base.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-07/c01 (audit migration), E-09/c01 (stats, health); other batches: E-15 (extends `accounts`), every epic with a tenant table (FK to `accounts`), E-22 (`migrate`, `serve` start check, `doctor`), E-21 (readiness)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

How the schema is versioned and changed, how the API refuses a mismatched schema, and the first table every tenant table references. Owner decision 2026-11-04: E-06 writes a minimal `accounts` migration (`id`, `state`, `created_at`) before any tenant table; E-15 adds the identity columns later.

Sources: `common-helpers/07_helper_db.md`, `epics/E-06_cross-cutting-database-access-and-migrations.md`, `data_model.md` §1, §4.1, §12.3, §13.1, `module-layout.md` § Migrations, AZ-3652 § Database Migration Approach.

## Shape

### Migrator and health

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `Migrator::run_pending` | `async fn run_pending(db: &Db) -> Result<MigrationReport, MigrationError>` (role `Migrator` only) | `MigrationFailed{version, error}`, `ChecksumMismatch`, `UnknownNewer` | async |
| `Migrator::revert` | `async fn revert(db: &Db, steps: u32) -> Result<MigrationReport, MigrationError>` (role `Migrator`) | `IrreversibleMigration`, as above | async |
| `Migrator::status` | `async fn status(db: &Db) -> MigrationStatus{applied, pending, dirty, unknown_newer}` (either role) | none | async |
| `Migrator::check_version` | `async fn check_version(db: &Db) -> SchemaState` (either role) | none | async |
| `Db::health` | `async fn health(&self) -> DbHealth{reachable, server_version, migrations_pending, pool_in_use}` | none | async |
| `Db::stats` | `fn stats(&self) -> DbStats` (read-only snapshot) | none | sync |

| Type | Shape |
|---|---|
| `MigrationReport` | `applied: Vec<version>` |
| `SchemaState` | `Current`, `Pending{n}`, `UnknownNewer` |
| `DbStats` | pool state (in use, idle, max), query durations by `op` label, pending migration count — the inputs of `db_up`, `db_pool_connections{state}`, `db_query_duration_seconds{op}`, `migrations_pending` (collected by E-09) |

Rules: migrations are embedded from `src/migrations/` as reversible pairs `YYYYMMDDHHMMSS_<verb>_<object>.up.sql` / `.down.sql`; each runs in one transaction under sqlx's advisory lock; applied files are immutable; a `down` that cannot undo raises `irreversible: restore from backup`; `-- no-transaction` migrations are idempotent. `serve` refuses to start unless `check_version` is `Current`.

### Schema contract: `accounts` base table (domain word `accounts_base`)

| Column | Type | Rule |
|---|---|---|
| `id` | `uuid` | primary key `accounts_pkey`; supplied by the application (UUIDv7), no `DEFAULT` |
| `state` | `text NOT NULL` | named check `accounts_state_check`: `active`, `disabled`, `deleting` |
| `created_at` | `timestamptz NOT NULL` | supplied by the application clock, no `DEFAULT now()` |

- Owned by `sunnycloud_migrator`; `sunnycloud` gets `SELECT, INSERT, UPDATE, DELETE` on it, granted in the same migration file; `sunnycloud` owns nothing.
- The same first migration grants `SELECT` on the migrations bookkeeping table to `sunnycloud`, so `check_version` works as the API role (helper 07 rule).
- No row-level security on `accounts` (keyed by `id`, not in the tenant list of data_model §12.3).
- Tenant tables reference `accounts (id)` with `ON DELETE RESTRICT` (data_model §12.2).
- E-15 adds `display_name`, `role`, `deletion_requested_at` and `accounts_state_idx` in its own `identity` migration.

## Invariants

- The API role cannot run DDL, so `run_pending` as `sunnycloud` fails; only the one-shot `migrate` step migrates.
- A killed migration leaves the earlier ones applied and its own changes rolled back; the next run retries from it.
- A database with a version the binary does not know is `UnknownNewer`, never treated as `Current`.
- `down` exists for every migration; up, down, up on an empty database succeeds.

## Non-Goals

- Not covered: the `migrate` and `serve` commands (E-22); readiness registration (E-21/E-22); the identity columns (E-15); the audit table (E-07); backups.

## Versioning Rules

- Applied migration files never change. A schema change is a new pair; additive by default (expand/contract). Renaming or dropping a column of `accounts` needs the owner's confirmation (data rules).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| first-migration | `run_pending` on empty PG 18 | `accounts` with the three columns, owner migrator, DML grants to `sunnycloud` | owner decision 2026-11-04 |
| state-check | insert `state = 'archived'` | `CheckViolation` | |
| up-down-up | apply all, revert all, apply all | each step succeeds | E-00 AC 6 |
| states | after run / with one pending file / with an unknown applied version | `Current` / `Pending{1}` / `UnknownNewer` | E-06 AC 6 |
| interrupted | kill during a migration | earlier applied, failed one rolled back, `Pending` | FT-N-39, AC-65 |
| api-role | `check_version` and `run_pending` as `sunnycloud` | first works, second fails | ADR-014 |

## Open points (flagged in the producer task)

1. E-15's later columns `display_name` and `role` are `NOT NULL` in data_model §4.1; adding NOT NULL columns to an existing table is allowed without a default only while no release exists. E-15 decides (default value or same pre-release window).
2. E-07 calls `create_audit_log` "the first real migration"; after the owner decision the `accounts_base` pair is first.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
