# ADR-014: Two database roles with row-level security as defence in depth

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Tenant isolation rests on application scoping (every query carries `account_id`), and row-level security (RLS) is a second layer. The API is the only process with a database login: workers talk to the API over `:8081` (S4-1), so the question is how many roles the API side needs and what each may do. The evaluator pass found that the first design (one role that owned the schema and ran the API, plus a `sunnycloud_worker` role for the queue) made RLS cosmetic: an owner bypasses RLS, the API could `ALTER TABLE ... DISABLE ROW LEVEL SECURITY` or re-grant itself anything, and `audit_log` append-only was only a convention. That single-role/worker-role design was superseded in the same planning round: the owner dropped the worker role (S4-1) and chose the migrator split (S4-2).

The forces: sqlx pools connections, so a session-level setting would leak to the next borrower; PostgreSQL bypasses RLS for owners and `BYPASSRLS` roles; some lookups (session by token hash, share token, login code) happen before the account is known; migrations rewrite data and must see all rows; the audit trail must survive a compromised API process.

- Acceptance criteria addressed: AC-59 (two-account matrix, 0 successful cross-account calls), AC-27 (account erase touches tenant tables through short batches), AC-65 (upgrade keeps all data; an interrupted migration rolls back and the server refuses to start)
- Restrictions addressed: RESTRICT-07 (PostgreSQL 18 as the tree store), RESTRICT-13 (workers hold no database credentials beyond the queue; later narrowed to none by S4-1), RESTRICT-32 (interrupted upgrade rolls back, server refuses to start)
- Risks addressed: R09 (tenant isolation rests on RLS plus application scoping), R10 (worker credential exposure)
- Research source: `_docs/00_research/raw/b_security.md` finding F24 (RLS caveats with pooling); `_docs/00_research/02_fact_cards/mve_server_media.md` (RLS facts at PG 18)

## Decision

We will run PostgreSQL with exactly two application roles: `sunnycloud_migrator`, which owns the schema, holds all DDL and runs the one-shot `sunnycloud migrate` Compose step (bypassing RLS as table owner), and `sunnycloud`, the API runtime role, which has DML only, owns nothing, is not superuser and has no `BYPASSRLS`. RLS is `ENABLE`d on every tenant table without `FORCE`; the policy binds `sunnycloud` because it is not the owner. The config keys are `db_api_url` and `db_migrator_url`; the migrator secret is not mounted in the API container. The API never migrates itself: at start it calls `Migrator::check_version` and refuses to serve on `Pending` or `UnknownNewer`. Backups run `pg_dump`/`pg_restore` as the Postgres superuser inside the db container.

Tenant scope is set per short transaction by the db helper (`sc_db`): `Db::run(Scope::Account(id), ...)` issues `set_config('sunnycloud.account_id', $id, true)` inside the same `DbTx`, so nothing leaks through the pool, and an unset value means no rows (fail closed). Cross-account work (admin endpoints, janitors, the CLI) uses `Db::run_system(op, ...)`, which sets `sunnycloud.scope = 'system'`. Lookups that happen before the account is known are named system-scope repository functions (session, device token, login code, OAuth state, share token, invite, all by hash). A CI test compares every system-scope call site with a reviewed allowlist, and a grep fails if `sunnycloud.scope` appears outside `sc_db`. `jobs` and `worker_status` are system-scope tables with no tenant policy, reachable only through 08.

`audit_log` is owned by the migrator; `sunnycloud` has `INSERT` and `SELECT` only. Retention deletes go through one `SECURITY DEFINER` purge function owned by the migrator and executable by `sunnycloud`; a CI test proves that `UPDATE` and `DELETE` as `sunnycloud` fail. `Db::connect` returns `WrongRole` when the API login is a superuser, has `BYPASSRLS` or owns the schema.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Single role that owns the schema and runs the API, with `FORCE ROW LEVEL SECURITY` | A compromised API could still disable RLS, re-grant privileges or edit `audit_log`: 1 role means 0 privilege boundaries between runtime and schema. `FORCE` also subjects migrations to policies, so data-rewriting migrations would need a second path anyway |
| Separate worker role for the job queue (first design) | Superseded by S4-1: workers have no database login, so a third credential would protect nothing. Removing it deletes one secret, one role test and the `db_worker_url` key |
| Schema-per-tenant | Search-path and migration cost grow with accounts (hundreds per install, RESTRICT-28): every migration would run N times; the cross-account janitors and system lookups still need a bypass. RLS needs one schema and one migration |
| No RLS, application scoping only | One missed `WHERE account_id` is then a cross-tenant leak with no second line. AC-59 covers every documented operation but not future code; RLS makes an unscoped query return 0 rows instead of foreign rows |

## Consequences

### Positive

- A compromised API process cannot alter the schema, re-grant privileges, switch off RLS or rewrite the audit log; an SQL injection is limited to DML under the current scope's policy.
- A forgotten scope fails closed (no rows), and the AC-59 matrix runs against the real `sunnycloud` role with RLS on, so the test exercises the production privilege set.
- Migration and runtime failures separate cleanly: a failed migration exits non-zero before the API starts, and the API's version check blocks start on a database newer than the binary.

### Negative

- Every transaction pays one `set_config`, and the policy adds a scalar subselect per statement; the 10,000-entry listing p95 of 300 ms (NFT-PERF-10) must be measured with RLS on.
- A bug in a system-scope function bypasses isolation by design; the reviewed allowlist and the small number of named functions are the only control.
- Data-rewriting migrations run as owner and see all rows, so each must state in its header which accounts it touches.
- Two secrets and a role-creation init script must be maintained; grants to `sunnycloud` ship in migrations.

### Neutral / Open

- RLS stays secondary: the generated two-account IDOR matrix (AC-59) remains the primary isolation test.
- `FORCE` is not needed and not used; adding it later would also constrain the migrator.
- `04_helper_audit.md` still says the API role owns `audit_log` and append-only is not a privilege boundary; that text predates S4-2 and `data_model.md` §10.1 wins (flagged for the reconciliation pass).

## Evidence

- `_docs/02_document/canonical-names.md` § Owner decisions at Step 4 (2026-11-04), S4-1 and S4-2
- `_docs/02_document/architecture.md` § 3. Deployment Model (runtime units `migrate`, `db`); § 7. Security Architecture (Authorization, Audit logging); § 9. Confirmed Design Answers (supersession note)
- `_docs/02_document/data_model.md` § 10.1 `audit_log` (proposal P3); § 12.3 Tenant isolation: `account_id` and row-level security; § 13.1 Migration strategy
- `_docs/02_document/common-helpers/07_helper_db.md` § Interface, § Rules, § Tests, § Risks
- `_docs/02_document/components/12_operator_cli/description.md` § 4. `migrate` and rollback behaviour; § 5. Composition root, scheduler, start-up and shutdown
- `_docs/02_document/risk_mitigations.md` § Risk Register (R09, R10)
- `_docs/00_problem/acceptance_criteria.md` AC-59, AC-65

## Notes

Re-evaluate `FORCE` or a third read-only reporting role only if a concrete need appears (for example an external BI export). Research finding F24 recommends a CI test that borrows a pooled connection after a request and asserts the scope setting is empty; `07_helper_db.md` § Tests does not list it yet (the transaction-local `set_config` is the design answer, the test is the proof). Treat it as a gap to close in the test specification, not as a new decision.
