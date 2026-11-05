# sc_db access: pool with role checks, short transactions, tenant scope, error mapping, gates

**Task**: AZ-3782_sc_db_access
**Name**: sc_db pool, roles, scopes, `DbTx`, error mapping and CI gates
**Description**: The only code that owns PostgreSQL connections: role-verified pools for the API and migrator roles, short scoped transactions with savepoints and the account advisory lock, SQLSTATE mapping, and the gates that keep scope handling and SQL text in their place
**Complexity**: 5 points
**Dependencies**: AZ-3763_sc_config, AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3741_docker_compose_skeletons, AZ-3652_initial_structure
**Component**: shared/db (helper 07)
**Tracker**: AZ-3782
**Epic**: AZ-3638 (E-06)
**Ref**: E-06/c01a

## Document Dependencies

- `_docs/02_document/contracts/shared_config/sc_config.md` — Database group keys and secrets, produced by E-01/c01.
- `_docs/02_document/contracts/shared_error/sc_error.md` — error taxonomy, produced by E-02/c01.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Timestamp`; the SQL `now()` gate, produced by E-03/c01.

## Problem

Tenant isolation rests on application scoping plus row-level security (R09), the API runs as a DML-only role (ADR-014), and long transactions across file I/O, HTTP calls or held long-polls are forbidden. If each component opened its own pool or set its own session variable, isolation and transaction length would drift per component.

## Outcome

- A pool refuses to start under the wrong role, and the API role is proven not to be superuser, `BYPASSRLS` or schema owner.
- Every unit of work runs in one short transaction with the tenant scope as its first statement; an unset scope sees no rows.
- Database errors arrive as typed variants callers can match; a lost connection is `Unavailable`.
- CI fails when scope settings, system scope or SQL text appear outside their allowed places.

## Scope

### Included
- `Db::connect` for the `Api` and `Migrator` roles with role verification (`WrongRole`, `Unavailable`), pool settings from the Database config group (`db_pool_max`, `db_acquire_timeout`, `db_idle_in_tx_timeout`, `db_statement_timeout`, `db_recovery_budget`).
- `Db::run` with `Scope::Account` and `Db::run_system(op, ...)`: scope set with `set_config(..., true)` first, commit on success, rollback on error or panic, connection returned with the scope cleared.
- `DbTx` wrapping the sqlx transaction (no public constructor), `DbTx::savepoint`, `DbTx::advisory_lock_account`.
- `DbError` mapping from SQLSTATE (23505, 23503, 23514, 40001, connection loss, 57P01, 08xxx).
- Pool recovery after the database returns.
- Gates: tenant and system scope settings appear only in this crate (grep); every `run_system` call site matches a reviewed allowlist; no SQL query text outside repository modules.
- Integration tests on a real PostgreSQL 18 container with both roles from the E-00/c11 init script.

### Excluded
- The `Migrator`, the first migration, `Db::health` and the stats snapshot (E-06/c01b).
- The SQL `now()` grep gate (E-03/c01 owns it).
- Table definitions (owning epics), the `migrate` command and the `serve` refusal wiring (E-22), backups.

## Acceptance Criteria

**AC-1: Wrong role refused (E-06 AC 2)**
Given a superuser role, a role with `BYPASSRLS`, a role that owns the schema and the real `sunnycloud` role
When `Db::connect(Api)` runs with each
Then the first three return `WrongRole` and the fourth connects; `Db::connect(Migrator)` connects only as `sunnycloud_migrator`

**AC-2: Cross-account reads return nothing (AC-59)**
Given two accounts and a test table with the documented row policy, RLS enabled, as role `sunnycloud`
When a `run` in scope A reads all rows, and a statement runs with no scope set
Then the first sees only A's rows and the second sees 0 rows

**AC-3: Scope never leaks (E-06 AC 4)**
Given a pool of 5 connections
When 1,000 `run` calls with alternating accounts finish
Then the tenant and system settings are empty on every connection returned to the pool

**AC-4: Rollback on error and panic**
Given a `run` closure that inserts a row and then returns an error, and one that panics after inserting
When both finish
Then neither row exists, the error is returned as the closure's error, and the pool keeps its size

**AC-5: Savepoint keeps the transaction usable**
Given a `run` that inserts a row, then runs a savepoint that violates a unique constraint, then inserts another row
When it commits
Then the savepoint returns `UniqueViolation{constraint}`, and both outer rows are committed

**AC-6: Short transactions (E-06 AC 5)**
Given `db_idle_in_tx_timeout` set to a short test value
When a transaction stays idle longer than that value
Then the server terminates it and the closure's next statement fails with `Unavailable`

**AC-7: Error mapping**
Given statements that raise SQLSTATE 23505, 23503, 23514 and 40001, and a killed backend connection
When they run inside `run`
Then they map to `UniqueViolation{constraint}`, `ForeignKeyViolation`, `CheckViolation`, `Serialization` and `Unavailable`

**AC-8: Recovery (E-06 AC 7)**
Given a running pool and a stopped database
When the database is started again
Then a `run` succeeds within `db_recovery_budget` (60 s by default) without restarting the process

**AC-9: Advisory lock serialises one account**
Given two concurrent `run` calls that take `advisory_lock_account` for the same account
When both run
Then the second waits until the first commits; for two different accounts they do not wait

**AC-10: Gates**
Given seeded violations: the scope setting name in another crate, a `run_system` call site not on the allowlist, and SQL text outside a repository module
When the gate tests run
Then each fails, and on the clean tree all pass

## Non-Functional Requirements

**Performance**
- Pool max 50 and acquire timeout 5 s by default; statement timeout 30 s; idle-in-transaction timeout 15 s.

**Security**
- System scope only through `run_system`; the API role has no DDL, no `BYPASSRLS`, is not superuser (R09, ADR-014).

**Reliability**
- No connection is held by a parked long-poll (callers never hold `DbTx` across a non-database await; review rule backed by the idle timeout).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-7 | SQLSTATE-to-variant table | Every listed code maps as stated; others `Other` |
| AC-10 | Grep and allowlist gates on seeded files | Fail on each violation |
| — | `compile_fail` doc test constructing `DbTx` | Does not compile |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | PG 18 with test roles | Role verification (integration, real PG 18) | As AC-1 | Security |
| AC-2 | Two accounts, RLS on | Cross-account read (integration); later NFT-RES-LIM-02 and NFT-SEC-13 through E-21 | 0 cross-account rows | Security |
| AC-3 | Pool of 5 | 1,000 runs (integration) | Settings empty | — |
| AC-6 | Short idle timeout | Idle transaction (integration) | Terminated | Performance |
| AC-8 | db stopped, then started | Recovery (integration) | `run` succeeds within the budget | Reliability |

## Constraints

- Real PostgreSQL 18 for every query-correctness test; no in-memory store.
- Scope settings per data_model §12.3 (`sunnycloud.account_id`, `sunnycloud.scope = 'system'`).
- Two roles only (S4-2); RLS `ENABLE` without `FORCE`.

## Risks & Mitigation

**Risk 1: A system-scope function bypasses isolation (R09)**
- *Risk*: a bug in a system-scope path reads other accounts.
- *Mitigation*: few named functions, reviewed allowlist, grep gate.

**Risk 2: Task size**
- *Risk*: the planned E-06 row (pool, scope, errors, migrator, gates) was already at the 5-point cap (E-06 risk 4) before the owner added the first migration.
- *Mitigation*: the row is split; this part has no migrator.

## Runtime Completeness

- Named capability: PostgreSQL 18 with row-level security and the two real roles.
- Production code that must exist: sqlx 0.9 pool and transactions against PostgreSQL 18; role checks against the server's catalog.
- Allowed external stubs: none.
- Unacceptable substitutes: an in-memory or SQLite store, a superuser test role standing in for `sunnycloud`, RLS turned off in tests.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Split of E-06's single 5-point row into c01a (this, 5 points) and c01b (migrator and first migration, 3 points): the row was at the cap before the owner decision of 2026-11-04 added the `accounts` migration. | Decompose parent | open | Low |
| `AccountId` is named by helper 07 but defined nowhere; ports type account ids as `Uuid`. Proposal: an alias of `Uuid`. | E-06 owner | open | Low |
| Helper 07 names only `sunnycloud.scope`; data_model §12.3 uses `sunnycloud.account_id` for tenants. This task follows the data model. | E-06 owner | open | Low |
| AZ-3654 (first-run pipeline) depends on `E-06/c01`; after the split it needs `E-06/c01b` (migrator), which depends on this task. | Decompose parent | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_db/sc_db_access.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
