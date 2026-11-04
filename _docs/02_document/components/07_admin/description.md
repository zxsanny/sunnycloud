# 07_admin — Users, invites, account deletion, last-admin protection

## 1. High-Level Overview

**Purpose**: (a) give admins the v1.0 user, invite, quota and health functions; (b) orchestrate account deletion across components within 15 minutes; (c) own the rule that the last active admin cannot be demoted, disabled or deleted.

**Split from the old `07_admin_quota`**: the quota ledger is now `07a_quota`. 07 sits above 01 and 07a and drives the `AccountLifecycleHook`s of the other components; it reaches all of them through `contracts` ports only.

**Architectural pattern**: orchestrator/facade over ports; no tables of its own. The deletion purger is a `Janitor` run by the scheduler in the `sunnycloud` binary (12), which also registers the hook list late (after construction): the composition root passes the lifecycle-hook list and the cleanup lists to the `DeletionPurger` constructor.

**Scope in**: admin user listing with usage, role/state/quota changes, the last-admin rule, admin invite routes (service owned by 01, routed under the admin group), account self-service view and deletion, health aggregation (`HealthContributor`), `admin_action` audit events, dev-seed.
**Scope out**: the quota ledger (07a), session cap and free-space floor (03/04), billing, organizations, user-facing trash, `/healthz` and `/metrics` (11), restore and scrub commands (12), the scheduler (12).

## 2. Ports (declared in `contracts`)

**Provides**
| Port | Methods | Consumers |
|---|---|---|
| `AdminService` | `list_users`, `update_user`, `set_quota`, `health` (take an `Actor`: `Admin(principal)` or `Operator`) | 11 (admin routes), 12 (`admin promote\|demote`, `admin user ...`) |
| `AccountService` | `view`, `quota_view` | 11 |
| `AccountDeletionService` | `request` | 11, 12 (`admin user delete`) |
| `Janitor` (impl `DeletionPurger`) | `name()`, `run_cycle(now)` | 12 scheduler |
| `DevSeed` | `seed` (refuses `environment = production`) | 12 |

**Requires**
| Port | Provider | Used for |
|---|---|---|
| `AccountAdmin` | 01_identity | `list_accounts`, `lock_active_admins(tx)`, `set_role(tx)`, `set_state(tx)`, `begin_deletion(tx)`, `finalize_deletion(tx)` |
| `AccountLifecycleHook` (list, registered late by 12) | 06, 02, 03, 08, 20, then 05, 07a | `erase_batch` until `remaining = 0`; the order is the registration order |
| `QuotaAdmin` | 07a_quota | usage in listings (`views`), `set_limit` (`InvalidLimit` bounds are defined in 07a) |
| `TreeReader` | 02_tree | `root_node` for the account view |
| `HealthContributor` (list) | any component | health aggregation |

Helpers: db, clock, error, audit.

## 3. Internal Interfaces

### AdminService (scope `Admin`, or `Actor::Operator` from the CLI)
`list_users(page) -> [AdminUserView{account, role, state, last_login, used, limit}]` (accounts from `AccountAdmin::list_accounts`, usage from `QuotaAdmin::views`; two calls, no cross-owner SQL). `update_user(account, role?, state?)` -> `LastAdmin` (HTTP 409), `NotFound`: one transaction that calls `AccountAdmin::lock_active_admins`, the guard, then `set_role` and/or `set_state` (01 revokes sessions and device tokens when it disables an account). `set_quota(account, limit)` -> `QuotaAdmin.set_limit`. `health() -> HealthReport` from the registered `HealthContributor`s (database, blob volume free space vs floor, master-key check, scrub result, dead jobs, janitor lag). Every mutating call writes `admin_action` (actor, subject id, action code; no names or emails).

### Last-admin protection (decision 15)
07 owns the rule. Role demotion, disabling and account deletion run in one short transaction: `AccountAdmin::lock_active_admins(tx)` takes the lock on the active admin rows, the guard refuses with `LastAdmin` when the change would leave none, then the mutating `AccountAdmin` calls (`set_role`, `set_state`, `begin_deletion`) run in the same transaction. Two concurrent demotions cannot both pass. The same guard serves the admin route, self-deletion and the operator commands.

### AccountService and AccountDeletionService
- `AccountService::view(principal) -> AccountView{id, display_name, role, identities, quota, root_node_id}`.
- `AccountDeletionService::request(principal, confirmation) -> DeletionAccepted{deadline}`; errors `Forbidden` (device token), `LastAdmin`, `ConfirmationMismatch`. One short transaction: `lock_active_admins` and the last-admin guard, then `begin_deletion` (state `deleting`; 01 revokes sessions, device tokens and grants). Nothing else happens in the request. `AccountGuard` makes later commits fail; held long-polls end within 5 s through 05's revalidation. `deadline` = now + `account_delete_deadline`.
- `DeletionPurger::run_cycle(now)` (a `Janitor`, every `account_delete_interval`): for each account in `deleting` (claimed with `FOR UPDATE SKIP LOCKED` on the `deleting` account row, so concurrent purger runs on one account are prevented), walk the hook list in registration order (12 registers 06 first so links stop at once, then 02, 03, 08, 20, and 05, 07a last because the tree's batches still use the feed and the quota row). For the current hook call `erase_batch(account, account_delete_batch_size)` until it reports `remaining = 0` or the cycle's time budget ends; then go to the next hook. Each `erase_batch` deletes rows and queues blobs in `blob_gc` with `Due::Now` in the same short transaction (data model account-deletion path) and holds no long transaction; the tree hook's `remaining` includes the account's blobs the GC sweeper has not removed yet. When every hook reports 0: one transaction with `finalize_deletion` (deletes `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row) and audit `account_deleted`. Idempotent per step; resumes by query after a crash. A failed batch is logged with account id and hook position and retried next tick.

### HealthContributor (trait in `contracts`, implemented by other components, registered by the composition root)
`name() -> &str`, `check() -> HealthItem{status ok|degraded|down, detail}`. 07 depends on the trait only; each check has a timeout.

## 4. External API Specification (resources owned; routed by 11_http_api)
| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/api/v1/accounts/me` | GET | SessionOrDevice | own account view |
| `/api/v1/accounts/me` | DELETE | SessionOnly | body `{confirmation}`; 202 with deadline |
| `/api/v1/accounts/me/quota` | GET | SessionOrDevice | caller's limit, used, reserved (from 07a) |
| `/api/v1/admin/users` | GET | Admin | paged list with usage |
| `/api/v1/admin/users/{id}` | PATCH | Admin | `{role?, state?}` |
| `/api/v1/admin/users/{id}/quota` | PUT | Admin | `{limit_bytes}` or null for default |
| `/api/v1/admin/health` | GET | Admin | aggregated component health |
Invites are routed under `/api/v1/admin/invites` (service owned by 01). Admin endpoints are in the generated two-account matrix (a non-admin gets 403, another account's id 404).

## 5. Data Access Patterns
**Owned**: none. Deletion progress is the account `state` plus the data that remains. No cache.
| Query | Frequency | Hot | Index |
|---|---|---|---|
| Admin list with usage | Low | No | accounts keyset (01), quota PK batch (07a) |
| Accounts in `deleting` | per purger tick | No | `accounts (state)` partial |

## 6. Implementation Details
**State**: stateless. **Deletion budget**: `account_delete_deadline` (15 min) target, metric `account_deletion_seconds`, alert after `account_delete_alert_after` (10 min). **Errors**: typed enums above; the purger logs a failed batch with IDs and retries next tick (best-effort loop, bounded by the 15 min alert), never swallowed silently. **Libraries**: none beyond helpers.

## 7. Configuration keys
`account_delete_batch_size` (5000), `account_delete_interval` (5 s), `account_delete_alert_after` (10 min), `account_delete_deadline` (15 min), `health_check_timeout`, `admin_list_page_size`. The purger drains until nothing is due within a time budget per cycle. Durations in config; tests use short values.

## 8. Security notes
Admin role is checked from the session principal on every call (`Scope::Admin`); device tokens never satisfy it. Deletion is session-only with explicit confirmation and last-admin protection (recent-sign-in requirement not added). Deleted data is removed immediately (retention does not apply); older backups still contain it (privacy text). No PII in audit `detail`. Dev-seed refuses production.

## 9. Dependency graph
Implement after the `contracts` ports and the providers' implementations in tests (fakes allowed only in unit tests; integration uses the real components). Blocks 11 admin routes and 12. Parallel: 05, 06.

## 10. ACs, tests, acceptance
**ACs [release]**: AC-27; AC-30 (account deletion is immediate); AC-21 (deleted account's streams end); AC-23 (admin management, last-admin); AC-1 (500 users via admin tooling, DS-29); AC-59 (admin routes in matrix) [all v1.0].
**Tests**: FT-P-65, FT-N-40; NFT-RES-30; NFT-SEC-20, NFT-SEC-27, NFT-SEC-13 (admin matrix); NFT-PERF-01 (500 accounts). Unit: guard under two concurrent demotions, purger resume after a simulated crash at each step.
**Acceptance (observable)**: deleting an account makes requests fail within 5 s and leaves zero rows, zero blob files and zero shares/sessions within 15 min, including after a crash mid-purge; the last admin cannot be removed by any route or command; admin health shows degraded when a component reports it.

## 11. Logging
ERROR purge batch failure (account id, step); WARN deletion past 10 min; INFO admin actions, deletion requested/completed (IDs). Structured JSON.

## 12. Risks
**Defaults chosen at Step 5 gap pass**: late registration is by constructor (composition root passes the hook and cleanup lists to `DeletionPurger`); `LastAdmin` maps to HTTP 409; concurrent purger runs on one account are prevented by claiming the `deleting` account row with `FOR UPDATE SKIP LOCKED` (the claim must not span the idle time of a cycle; see short-transaction rule); `InvalidLimit` bounds are in 07a.
Large-account deletion time depends on 04's GC throughput. Health aggregation must tolerate a slow contributor (timeout per check). A hook missing from the late-registered list leaves rows behind and blocks nothing visibly; NFT-SEC-27 counts rows. Hook order matters: 05 and 07a come last because 02's batches still stamp and adjust usage. Large erasures are bounded by `account_delete_batch_size` per transaction.
