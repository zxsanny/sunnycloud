# Helper: DB (pool, short transactions, scopes, roles, migrations)

**Crate**: `sc_db`. **Used by**: every server component that touches PostgreSQL (01 to 08), 12_operator_cli (`migrate`, system scope), 11_http_api (readiness). The worker supervisor (09) does not use it: workers have no database login (S4-1). `contracts` depends on this crate only for the `DbTx` type in port signatures.

## Purpose
One place that owns connections, transaction shape and tenant scope, so no component opens a pool, sets a session variable or runs a migration on its own. Transactions are short: open, do the unit of work, commit or roll back, release. Nothing holds a transaction across a file operation, an HTTP call, a sleep or a held long-poll.

## Interface

| Operation | Input | Output | Errors |
|-----------|-------|--------|--------|
| `Db::connect` | `DbConfig`, `DbRole::{Api, Migrator}` | `Db` (pool; URL from secret `db_api_url` or `db_migrator_url`) | `DbError::Unavailable`, `DbError::WrongRole` (connected user does not match the requested role, or the API role is a superuser, has `BYPASSRLS` or owns the schema) |
| `Db::run` | `Scope::Account(AccountId)`, async closure `FnOnce(&mut DbTx) -> Result<T, E>` | `T` | `E` from the closure (rolled back), `DbError` |
| `Db::run_system` | `op: &'static str`, async closure | `T` | as `run`; sets `sunnycloud.scope = 'system'` with `set_config(..., true)` as the first statement; `op` is the metric label |
| `DbTx::savepoint` | async closure | `T`; a typed failure inside rolls back to the savepoint and leaves the outer transaction usable | as the closure |
| `DbTx::advisory_lock_account` | `AccountId` | `()` (`pg_advisory_xact_lock`, released at commit) | none |
| `DbError` mapping | SQLSTATE | `UniqueViolation{constraint}` (23505), `ForeignKeyViolation` (23503), `CheckViolation` (23514), `Serialization` (40001), `Unavailable` (connection, 57P01, 08xxx), `Other` | callers match the variant and return their own typed result |
| `Migrator::run_pending` | `Db` (role `Migrator`) | `MigrationReport{applied}` | `MigrationFailed{version, error}`, `ChecksumMismatch`, `UnknownNewer` |
| `Migrator::revert` | `Db` (role `Migrator`), steps | `MigrationReport` | `IrreversibleMigration` |
| `Migrator::status` | `Db` (either role) | `applied`, `pending`, `dirty`, `unknown_newer` | none |
| `Migrator::check_version` | `Db` (either role) | `SchemaState::{Current, Pending{n}, UnknownNewer}` | none; used by `serve` (refuses to start unless `Current`), `/readyz` and `doctor` |
| `Db::health` | none | `{reachable, server_version, migrations_pending, pool_in_use}` | none |

`DbTx` cannot be constructed outside this crate. Ports that must join a caller's transaction take `&mut DbTx`; everything else opens its own `Db::run`.

## Rules
- **Roles (S4-2)**: two application roles. `sunnycloud_migrator` owns the schema and is the only role with DDL; it bypasses row-level security as table owner. It is used by `sunnycloud migrate` (run as a one-shot step before the API starts) and by nothing else; its secret `db_migrator_url` is not mounted in the API container. `sunnycloud` is the API runtime role: DML only, no DDL, no `BYPASSRLS`, not superuser, `SELECT` on the migrations table. On `audit_log` it has `INSERT` and `SELECT` only; the retention purge is a `SECURITY DEFINER` function owned by the migrator. The init script creates both roles from secret files on an empty data directory; later grants are in migrations. There is no worker role and no decoder database access. Backups run `pg_dump`/`pg_restore` as the Postgres superuser inside the db container.
- **Row-level security**: tenant tables have RLS `ENABLE`d (no `FORCE`); the policy binds `sunnycloud` because it is not the owner. Unset scope means no rows. Data-rewriting migrations run as the owner and see all rows.
- **System scope**: reachable only through `Db::run_system(op, ...)`. A CI test compares every call site with a reviewed allowlist of repository modules, and a grep fails if the string `sunnycloud.scope` appears outside `sc_db`. System scope is for admin endpoints, janitors, the CLI, and lookups that happen before the account is known. Those lookups are named system-scope repository functions: session by token hash, device token by hash, login code by hash, OAuth state by hash, share token by hash, invite by hash (01, 06). `jobs` and `worker_status` (08) are system-scope tables with no tenant policy: only 08's queue repository touches them, and the claim has no account yet.
- **Short transactions**: the pool sets `idle_in_transaction_session_timeout` (`db_idle_in_tx_timeout`, default 15 s) and `statement_timeout` (`db_statement_timeout`, default 30 s). Janitor cycles use one transaction per batch. No `await` on a non-database call inside the closure; this is a review rule, backed by the idle timeout.
- **No SQL `now()`**: callers pass `Timestamp` from the clock. A grep test enforces it.
- **Migrations**: sqlx, forward by default, run by `sunnycloud migrate` with `db_migrator_url`; the API does not migrate itself. Each migration runs in one transaction under sqlx's advisory lock; a failure or kill rolls that migration back and the command exits non-zero. `-- no-transaction` migrations are idempotent. `serve` calls `check_version` and refuses to start on `Pending` or `UnknownNewer`. `down` files exist for CI (up, down, up) and for `migrate revert`; operator rollback after a committed migration is a restore, not `revert`.
- **Queries**: compile-time checked sqlx with `SQLX_OFFLINE`; no query text outside repository modules; a listing or feed query that needs a plan guarantee has a CI plan check.
- **Pool**: `db_pool_max` (default 50), `db_acquire_timeout` (5 s). Held long-polls hold no connection (05).
- **Metrics**: `db_up`, `db_pool_connections{state}`, `db_query_duration_seconds{op}`, `migrations_pending`.

## Tests
FT-N-39 and NFT-RES-14 (interrupted migration), a pooled-connection test (after a request the tenant scope setting is empty on the returned connection), AC-59 matrix with the real `sunnycloud` role and RLS on, grep tests (system-scope string, no `now()`), role privilege test (`sunnycloud` cannot run DDL, has no `BYPASSRLS`, cannot `UPDATE` or `DELETE` `audit_log`; migrator-owned tables are not owned by `sunnycloud`), system-scope allowlist test, `check_version` refusal test (pending and newer), savepoint test (a failed batch entry leaves the transaction usable), idle-timeout test.

## Risks
- A bug in a system-scope function bypasses tenant isolation by design; the reviewed allowlist and the small number of named functions are the control.
- The API role cannot change the schema or the audit log, so an SQL injection is limited to DML on tenant data under the policy of the current scope.
- A data-rewriting migration runs as owner and bypasses RLS: each such migration states in its header which accounts it touches.
