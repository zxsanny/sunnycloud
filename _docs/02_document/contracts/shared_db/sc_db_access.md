# Contract: sc_db access — pool, roles, short transactions, tenant scope, error mapping

**Component**: shared/db (crate `sc_db`)
**Producer task**: E-06/c01a — `_docs/02_tasks/todo/B01-16_sc_db_access.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-06/c01b, E-07/c01; E-00/c03..c05 (`DbTx` only); other batches: 01-08, 07a, 12, 20
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The only code that owns PostgreSQL connections, transaction shape and tenant scope. Components never open a pool, set a session variable or keep a transaction across I/O. The migrator, health and stats are in `sc_db_migrations.md` (E-06/c01b).

Sources: `common-helpers/07_helper_db.md`, `epics/E-06_cross-cutting-database-access-and-migrations.md`, `data_model.md` §12.3, ADR-014, `canonical-names.md` S4-2.

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `Db::connect` | `async fn connect(cfg: &DbConfig, role: DbRole) -> Result<Db, DbError>` (URL from the secret `db_api_url` or `db_migrator_url`) | `Unavailable`, `WrongRole` | async |
| `Db::run` | `async fn run<T, E>(&self, scope: Scope, f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>) -> Result<T, RunError<E>>` | the closure's `E` (rolled back), `DbError` | async |
| `Db::run_system` | `async fn run_system<T, E>(&self, op: &'static str, f) -> Result<T, RunError<E>>` | as `run` | async |
| `DbTx::savepoint` | `async fn savepoint<T, E>(&mut self, f) -> Result<T, E>` (a failure rolls back to the savepoint; the outer transaction stays usable) | the closure's `E` | async |
| `DbTx::advisory_lock_account` | `async fn advisory_lock_account(&mut self, account: AccountId) -> Result<(), DbError>` (`pg_advisory_xact_lock`, released at commit) | `DbError` on connection loss only | async |

| Type | Shape |
|---|---|
| `DbRole` | `Api` (role `sunnycloud`), `Migrator` (role `sunnycloud_migrator`) |
| `DbConfig` | the Database group of `sc_config`: `db_api_url` / `db_migrator_url` (secrets), `db_pool_max` (50), `db_acquire_timeout` (5 s), `db_idle_in_tx_timeout` (15 s), `db_statement_timeout` (30 s), `db_recovery_budget` (60 s) |
| `Scope` | `Account(AccountId)`; system scope is reachable only through `run_system` |
| `AccountId` | the account `Uuid` (see Open points) |
| `DbTx` | the open transaction; no public constructor; handed out only by `run`, `run_system` and `savepoint` |
| `DbError` | `UniqueViolation{constraint}` (23505), `ForeignKeyViolation` (23503), `CheckViolation` (23514), `Serialization` (40001), `Unavailable` (connection loss, 57P01, 08xxx), `WrongRole`, `Other` |
| `RunError<E>` | `Closure(E)` or `Db(DbError)` |

Role verification at `connect`: the connected user matches the requested role; for `Api` the role is not a superuser, has no `BYPASSRLS` and does not own the schema; otherwise `WrongRole`.

Scope settings (data_model §12.3): `run` sets the tenant setting `sunnycloud.account_id` and `run_system` sets `sunnycloud.scope = 'system'`, each with `set_config(..., true)` as the first statement of the transaction; an unset scope means the row policies return no rows.

## Invariants

- These setting names appear in no other crate (grep gate). `run_system` call sites match a reviewed allowlist (CI test). No SQL query text outside repository modules (grep gate).
- A transaction never outlives its closure: commit on `Ok`, rollback on `Err` or panic; the connection returns to the pool with the scope cleared.
- No closure awaits a non-database call (review rule, backed by the idle-in-transaction timeout).
- The pool recovers within `db_recovery_budget` after the database returns.
- `DbTx` cannot be constructed outside the crate (`compile_fail` doc test); `contracts` imports `DbTx` and nothing else from here.

## Non-Goals

- Not covered: the migrator, the first migration, `Db::health` and the stats snapshot (E-06/c01b); table definitions (owning epics); the role init script (E-00/c11); the SQL `now()` gate (owned by E-03).

## Versioning Rules

- `DbTx` crosses ports as `&mut DbTx`; changing that or `Scope` is breaking for every component. New `DbError` variants are breaking for callers that match exhaustively (they must keep a catch-all `Other` arm).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| wrong-role | `connect(Api)` as superuser and as a `BYPASSRLS` role | `WrongRole` | E-06 AC 2 |
| scope-reset | 1,000 pooled `run` calls | tenant setting empty on every returned connection | E-06 AC 4 |
| cross-account | two accounts, a test table with the documented policy | scope A reads 0 rows of B | AC-59 |
| savepoint | failed savepoint inside `run`, then a statement | outer transaction usable, commit succeeds | |
| idle-kill | a transaction idle beyond `db_idle_in_tx_timeout` | terminated by the server | E-06 AC 5 |
| sqlstate | violations 23505, 23503, 23514, 40001 | mapped variants | |

## Open points (flagged in the producer task)

1. `AccountId` is named by helper 07 but defined nowhere; the port crates type account ids as `Uuid`. Proposal: `AccountId` is an alias of `Uuid` so `Scope::Account` takes the same value.
2. Helper 07 names the tenant setting `sunnycloud.scope` only; data_model §12.3 uses `sunnycloud.account_id` for tenants and `sunnycloud.scope = 'system'` for system scope. This contract follows the data model.
3. The SQL `now()` grep gate is listed by both E-03 and E-06; it is built once, by E-03.
4. Helper 07 says `run` returns the closure's `E` or a `DbError`; the wrapper name `RunError<E>` is introduced here for that pair.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
