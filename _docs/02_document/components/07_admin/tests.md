# Test Specification — 07 Admin

Scope: ports `AdminService`, `AccountService`, `AccountDeletionService`, `Janitor` (`DeletionPurger`), `DevSeed`, and the `HealthContributor` aggregation. Names follow `canonical-names.md`. 07 owns no tables, so every check reads rows through the harness superuser connection (assert/arrange only; the SUT never uses it). System-level scenarios are cited by ID and not restated. 07 owns no interaction AC (AC-68..80); it contributes to none by name, so none appears below.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-27 | Account deletion removes files, thumbnails, transcodes, shares, sessions; done within 15 min | IT-07-9, IT-07-10, IT-07-11, IT-07-12, PT-07-2, AT-07-1; sys FT-P-19, NFT-RES-30, NFT-SEC-27 | Covered by this file and system scenarios |
| AC-30 | Account deletion is immediate (retention does not apply) | IT-07-9, AT-07-1; sys FT-P-19 | Covered by this file and system scenarios |
| AC-21 | Revoked or deleted account stops syncing within 5 s | IT-07-8, IT-07-3; sys FT-N-30, NFT-SEC-27 | Covered by this file (revocation at the port: sessions and device tokens rejected by `Authenticator` after `request`); the 5 s live-stream cut is 05's revalidation, covered by sys FT-N-30 |
| AC-23 | Admin management; the last active admin cannot be demoted, disabled or deleted | IT-07-3, IT-07-4, IT-07-5, ST-07-3, AT-07-2; sys FT-P-65, FT-N-20, FT-N-21 | Covered by this file and system scenarios |
| AC-1 | 500 registered users handled through admin tooling (DS-29) | PT-07-1; sys NFT-PERF-01 | Covered by this file (listing part) and sys NFT-PERF-01 |
| AC-59 | Admin routes in the two-account matrix: non-admin 403, other account 404 | ST-07-1, ST-07-2; sys NFT-SEC-13 | Covered by this file (port level) and sys NFT-SEC-13 (HTTP level) |
| AC-57 | Device tokens cannot call delete-account | ST-07-2; sys NFT-SEC-08 | Covered by this file and sys NFT-SEC-08 |

AC-68..AC-80: none names 07 as owner or contributor (`interaction-risks.md`, section "Component-owned interaction ACs"). No AC is uncovered.

## Blackbox Tests

Harness `H1`: Rust integration binary calling the 07 ports, real PostgreSQL 18 (roles `sunnycloud_migrator`, `sunnycloud`, RLS on), real 01, 02, 03, 04, 05, 06, 07a, 08 implementations, hooks registered through the late-registration seam that 12 uses (the method name is not in the description, see gap G-07-1). Config: `account_delete_batch_size=50`, `account_delete_interval=200ms`, `account_delete_deadline=30s`, `account_delete_alert_after=20s`, `health_check_timeout=300ms`, `admin_list_page_size=20`.

### IT-07-1: User listing with usage and paging
**Summary**: `list_users(page)` returns accounts from `AccountAdmin::list_accounts` and usage from `QuotaAdmin::views`, paged by `admin_list_page_size`.
**Traces to**: AC-1, AC-23
**Input data**: 45 accounts (3 admins, 2 disabled, 1 with limit 1 GiB, used 300 MiB, reserved 100 MiB); pages requested until exhausted.
**Expected result**: pages of 20, 20, 5; every account exactly once, no duplicate across pages; each `AdminUserView` has `role`, `state`, `last_login`, `used`, `limit` equal to the harness-seeded values; no password-like or token fields; page order is stable across two runs.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL, 01, 07a

### IT-07-2: Role, state and quota changes
**Summary**: `update_user` and `set_quota` apply changes, write `admin_action` rows with ids only, and a disabled account loses its sessions and device tokens.
**Traces to**: AC-23, AC-21
**Input data**: admin A, user U with 2 sessions and 1 device token; `update_user(U, role=admin)`, `update_user(U, state=disabled)`, `set_quota(U, 5 GiB)`, `set_quota(U, None)`.
**Expected result**: role and state change in one transaction; after disabling, `Authenticator` rejects all 3 of U's credentials; quota view shows 5 GiB, then the installation default; 4 `admin_action` rows (actor A, subject U id, action code), none containing display name or email (substring scan); unknown id returns `NotFound` with no row written.
**Max execution time**: 5 s
**Dependencies**: 01, 07a, helper audit

### IT-07-3: Last-admin rule on every mutation
**Summary**: demote, disable and delete of the only active admin each return `LastAdmin` and change nothing.
**Traces to**: AC-23
**Input data**: one active admin A plus two ordinary users and one already-disabled admin D (a disabled admin does not count as active). Calls: `update_user(A, role=user)`, `update_user(A, state=disabled)`, deletion of A (`AccountDeletionService::request(A session, A's id text)` for `Actor::Admin(A)`, `request_as_operator(A)` for `Actor::Operator`), each as `Actor::Admin(A)` and `Actor::Operator`.
**Expected result**: six `LastAdmin` errors; A's role, state and sessions unchanged; after promoting another user to admin, the same demote of A succeeds.
**Max execution time**: 5 s
**Dependencies**: 01

### IT-07-4: Concurrent demotions cannot both pass
**Summary**: two admins demote each other in parallel; the lock from `lock_active_admins` serializes the guard.
**Traces to**: AC-23
**Input data**: 100 rounds; each round creates admins A and B (only active admins) and runs `update_user(B, role=user)` as A and `update_user(A, role=user)` as B on two connections released by a barrier.
**Expected result**: in every round exactly one call succeeds and one returns `LastAdmin`; the active-admin count after every round is 1; 0 deadlock errors surfaced to the caller (retry inside the port is allowed, at most once).
**Max execution time**: 30 s
**Dependencies**: 01

### IT-07-5: Concurrent delete, disable and demote mix
**Summary**: self-deletion, disable and demote races on a 2-admin set never leave zero active admins.
**Traces to**: AC-23
**Input data**: 200 rounds with admins A, B; random pairs from {request deletion of A, request deletion of B, disable B, demote A, demote B} run concurrently.
**Expected result**: after each round at least 1 account with role admin and state active; every rejected call is `LastAdmin`; an account in `deleting` is not counted as active.
**Max execution time**: 60 s
**Dependencies**: 01

### IT-07-6: Health aggregation
**Summary**: `health()` returns the registered `HealthContributor` results and tolerates a slow or failing contributor.
**Traces to**: AC-27 (observability of deletion lag), AC-23
**Input data**: contributors database (ok), blob volume (degraded), master key (ok), scrub (ok), dead jobs (down), janitor lag (ok), plus a test contributor sleeping 5 s and one returning an error.
**Expected result**: report lists each by `name()` with `ok|degraded|down` and `detail`; overall is not ok; the sleeping contributor is reported `down` (or timed-out detail) within `health_check_timeout` + 200 ms and the others are still present; whole call returns in under 1 s.
**Max execution time**: 3 s
**Dependencies**: 04, 08, 12 registration seam

### IT-07-7: AccountService views
**Summary**: `view` and `quota_view` return the caller's own data only.
**Traces to**: AC-59
**Input data**: accounts A (Google identity, GitHub identity) and B; calls with A's and B's principals.
**Expected result**: `AccountView{id, display_name, role, identities, quota, root_node_id}` of A has both identities and the root node id equal to `TreeReader::root_node(A)`; `quota_view` equals `QuotaAdmin::view(A)` (limit, used, reserved); B's call never returns A's values.
**Max execution time**: 3 s
**Dependencies**: 01, 02, 07a

### IT-07-8: Deletion request
**Summary**: `request` validates, marks the account `deleting`, revokes credentials and returns the deadline; nothing else happens in the request.
**Traces to**: AC-27, AC-21, AC-30
**Input data**: user U with files, a share, 2 sessions, 1 device token; calls with a device token, wrong confirmation text, correct confirmation (U's own account id as lowercase hyphenated UUID text) as session; then `request_as_operator` on a second user V.
**Expected result**: device token gives `Forbidden`, wrong text `ConfirmationMismatch`, state unchanged; correct call returns `DeletionAccepted{deadline = now + 30 s}` within 1 s; U's state is `deleting`; all U's credentials rejected; U's rows (nodes, blobs, shares) still exist immediately after the call (the purger removes them later); a commit attempt by U fails through `AccountGuard`; `admin_action`/audit row for the request has ids only; `request_as_operator(V)` needs no confirmation and gives the same end state for V.
**Max execution time**: 5 s
**Dependencies**: 01, 02, 03, 06

### IT-07-9: Purger drains every hook and finalizes last
**Summary**: `DeletionPurger::run_cycle` calls `erase_batch` per hook in registration order until `remaining = 0`, then `finalize_deletion`.
**Traces to**: AC-27, AC-30
**Input data**: account with 10,000 live files in 200 folders, 400 retained revisions, 50 shares, 3 upload sessions with staged rows and reservations, 120 feed rows, 30 photo/video jobs and 60 derived rows (DS-03 subset), a quota row; hooks registered in order 06, 02, 03, 08, 20, 05, 07a; recording wrapper around each hook (records order and `remaining` values; delegates to the real hook).
**Expected result**: `erase_batch` is never called with limit above 50; hook N+1 is first called only after hook N reported 0; `finalize_deletion` runs once, last; afterwards the harness table sweep (every table with an `account_id` column from `information_schema`, plus `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes`, `accounts`) counts 0 rows for the account; `blob_gc` holds the account's blobs with `Due::Now`; after 04's GC runs, 0 blob files remain for it; audit `account_deleted` written once; other accounts' row counts unchanged.
**Max execution time**: 30 s (deadline config)
**Dependencies**: all hook owners, 04 GC

### IT-07-10: Resume after a failed batch at each hook position
**Summary**: a batch failure at any step is logged and retried; the purge finishes without manual action.
**Traces to**: AC-27
**Input data**: same account as IT-07-9 (smaller: 500 files); a decorator around hook k in {1..7} (including `finalize_deletion` as step 8) fails the first call with an error and delegates afterwards; run 8 rounds, one per k; a ninth round drops the whole cycle after a partial batch (task cancelled mid-cycle) and starts a fresh purger instance.
**Expected result**: failure logged at ERROR with account id and hook position (no row data); next `run_cycle` continues from the remaining data (no duplicate `erase_batch` effects, no error from already-deleted rows); final sweep 0 rows in each round; no second `account_deleted` audit row; account remains `deleting` until the last step.
**Max execution time**: 60 s total
**Dependencies**: all hook owners

### IT-07-11: Cycle time budget and batch bound
**Summary**: a cycle stops at its time budget and the next cycle resumes; no batch exceeds the configured size.
**Traces to**: AC-27
**Input data**: account with 5,000 files, `account_delete_batch_size=50`, cycle budget 300 ms (config), hook decorator recording batch sizes and transaction durations.
**Expected result**: more than 1 cycle needed; each `erase_batch` call with limit 50; no single transaction open longer than 1 s (measured by `pg_stat_activity` sampling every 50 ms: no `xact_start` older than 1 s attributable to the purger); end state same as IT-07-9.
**Max execution time**: 60 s
**Dependencies**: 02, 03, 04

### IT-07-12: Two purgers overlap
**Summary**: two `run_cycle` calls on the same account in parallel end in the same clean state.
**Traces to**: AC-27
**Input data**: account with 2,000 files; two purger instances (as two API processes would be during a rolling restart) call `run_cycle` in a loop on separate connections.
**Expected result**: final sweep 0 rows; zero unhandled errors (a lost race is a retried tick); exactly one `account_deleted` audit event. Rule (resolved 2026-11-07, G-07-2): each purger step takes a per-step `FOR UPDATE SKIP LOCKED` claim on the `deleting` account row through `AccountAdmin::lock_deleting(tx, account)`; a purger that gets `false` skips the step, and two purgers alternating steps on one account is harmless because the hooks are restart-safe by query alone.
**Max execution time**: 60 s
**Dependencies**: all hook owners

### IT-07-13: Janitor with nothing to do
**Summary**: `run_cycle` with no `deleting` account does no writes.
**Traces to**: AC-27
**Input data**: stack with 20 active accounts; 100 consecutive `run_cycle(now)` calls; `name()` queried.
**Expected result**: `name()` is a stable non-empty string; `pg_stat_user_tables` insert/update/delete deltas are 0 for all tables; each cycle returns in under 100 ms.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL

### IT-07-14: DevSeed
**Summary**: `seed` creates demo data outside production and refuses in production.
**Traces to**: AC-23
**Input data**: config `environment = development`, then `environment = production`.
**Expected result**: development: seed returns Ok, at least 1 admin and 1 user exist, a second `seed` call adds no duplicates (or reports already seeded); production: error, 0 rows written (row counts of `accounts`, `nodes` unchanged).
**Max execution time**: 10 s
**Dependencies**: 01, 02

## Performance Tests

### PT-07-1: Admin listing with 500 accounts
**Summary**: `list_users` over DS-29 accounts stays fast (two calls, no cross-owner SQL).
**Traces to**: AC-1
**Load scenario**:
- Concurrent users: 4 admin callers
- Request rate: 20 calls/s total
- Duration: 2 minutes
- Ramp-up: none (DS-29 `golden+accounts-500` loaded)

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Latency (p95) per page of 20 | 30 ms | 100 ms |
| Latency (p99) | 60 ms | 200 ms |
| Error rate | 0% | 0% |

**Resource limits**:
- Database connections: at most 4 active (queries per call: 2)

The targets are proposed (the description sets none) and replaced by a measured baseline at the first run.

### PT-07-2: Deletion duration of a large account
**Summary**: a large account is fully erased within the AC-27 limit at production batch settings.
**Traces to**: AC-27
**Load scenario**:
- Account: 100,000 files, 1,000 shares, 100 sessions, 5,000 derived rows (generated through the API, `gen_tree.py --count 100000`)
- Config: production defaults (`account_delete_batch_size=5000`, `account_delete_interval=5s`)
- Background load: 20 other accounts doing small-file commits (1 per second each)

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Request to finalize | 5 min | 15 min |
| Longest purger transaction | 2 s | 10 s |
| Other accounts' commit p95 | within 20% of baseline | 2x baseline |

**Resource limits**:
- Database connections: purger uses at most 2. Runs on C-REF hardware; elsewhere timings are `NOT-RUN(HW-REF)`.

## Security Tests

### ST-07-1: Non-admin and foreign calls to AdminService
**Summary**: admin operations are refused to non-admin principals and device tokens.
**Traces to**: AC-59
**Attack vector**: privilege escalation by ordinary session and by device token calling `list_users`, `update_user`, `set_quota`, `health`.
**Test procedure**:
1. As ordinary user U: call each operation with `Actor::Admin(U)`.
2. As admin A's device token: call each operation.
3. As admin A, call `update_user` and `set_quota` with an id that does not exist and with a random UUID.
**Expected behavior**: steps 1 and 2 return `Forbidden` (HTTP 403 at the route, G-S6 resolved 2026-11-07); step 3 returns `NotFound` (HTTP 404) with no distinction between "exists but other" and "missing" (accounts have no owner notion, so the only 404 case is absence).
**Pass criteria**: 0 successful mutations; 0 rows written to `admin_action` for refused calls except an optional refusal event with ids only.
**Fail criteria**: any role, state or limit change by a non-admin.

### ST-07-2: Deletion is session-only with confirmation
**Summary**: a device token or a missing confirmation cannot start a deletion.
**Traces to**: AC-57, AC-27
**Attack vector**: stolen sync token deleting the account.
**Test procedure**:
1. Call `request` with a device token and the right confirmation.
2. Call with a session and empty, whitespace, and wrong-case confirmation.
3. Call with a session for another account's id in the confirmation.
**Expected behavior**: `Forbidden` for step 1, `ConfirmationMismatch` for steps 2 and 3. The right confirmation is the caller's own account id as lowercase hyphenated UUID text, compared for exact equality (no trimming, no case folding); empty, whitespace-only, an upper-case rendering of the own id, and another account's id each give `ConfirmationMismatch`. The operator path `request_as_operator` has no confirmation and is covered by IT-07-3 (last-admin) and IT-07-8.
**Pass criteria**: account state stays `active`; credentials still valid.
**Fail criteria**: any state change.

### ST-07-3: Admin demotion cannot be raced around the guard
**Summary**: the last-admin rule holds under injected delay between guard and write.
**Traces to**: AC-23
**Attack vector**: two operator and admin callers interleaving (CLI plus HTTP) with a test delay hook inside the transaction.
**Test procedure**: run IT-07-4 with a 200 ms delay injected through a `tokio::time` pause in the `AccountAdmin` decorator after `lock_active_admins` returns.
**Expected behavior**: the second transaction waits on the lock; exactly one passes.
**Pass criteria**: active admin count is 1 after every round.
**Fail criteria**: count 0.

### ST-07-4: No PII in audit and logs
**Summary**: `admin_action` and purger logs carry ids and codes only.
**Traces to**: AC-27
**Attack vector**: information disclosure through audit export and logs.
**Test procedure**: run IT-07-2 and IT-07-9 with accounts named `Zed Unique-7Q`, email `zed.unique7q@example.test`; scan `audit_log.detail`, the JSON logs and metrics output for those strings.
**Expected behavior**: no occurrence.
**Pass criteria**: 0 matches.
**Fail criteria**: any match.

## Acceptance Tests

### AT-07-1: Deleting an account leaves nothing behind
**Summary**: observable end state of account deletion, including after a crash mid-purge.
**Traces to**: AC-27, AC-30, AC-21
**Preconditions**:
- Compose stack (profile `core`), account with DS-23 files, a DS-03 photo with finished thumbnail, a share link, 2 sessions; purge interval short.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `DELETE /api/v1/accounts/me` with confirmation (session) | 202 with `deadline` |
| 2 | Use the old session and device token | 401 within 5 s |
| 3 | `docker kill api` once, `docker start api` | purge resumes |
| 4 | Poll ctl read-only probe until deletion finished | within 15 min (target 60 s) 0 rows, 0 blob files, 0 shares for the account; share link answers as missing |

Sys scenario FT-P-19 / NFT-RES-30 repeat this with the full client set; this test is the component-level check of 07.

### AT-07-2: The last admin cannot be removed by any route
**Summary**: no admin route, account route or operator command removes the last active admin.
**Traces to**: AC-23
**Preconditions**: stack with exactly one active admin A.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `PATCH /api/v1/admin/users/{A}` with `role=user`, then `state=disabled` | 409/422-class refusal with code `last_admin` (status per 11 mapping); A unchanged |
| 2 | `DELETE /api/v1/accounts/me` as A | refusal `last_admin` |
| 3 | `sunnycloud admin demote A`, `admin user delete A --yes` | exit 2 both |
| 4 | Admin health in `GET /api/v1/admin/health` with a failing contributor | report shows `degraded` |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| accounts-45 | 45 accounts, mixed roles/states, quota rows | generated by harness factory | < 1 MB |
| tree-small | 10,000 files in 200 folders, 400 retained revisions, 50 shares | DS-01 subset via the API | about 100 MB |
| tree-large | 100,000 files (PT-07-2) | `gen_tree.py --count 100000` | about 0.5 GB |
| accounts-500 | DS-29 snapshot `golden+accounts-500` | created by ctl | about 50 MB DB |
| media-subset | DS-03 photos (10) for jobs and derived rows | DS-03 | about 40 MB |

**Setup procedure**:
1. Start PostgreSQL 18 (pinned digest) in a container; run `sunnycloud migrate` as `sunnycloud_migrator` into template database `sc_template`.
2. For each test: `CREATE DATABASE t_<id> TEMPLATE sc_template`; connect the SUT as role `sunnycloud`; the harness superuser connection is used only to arrange and assert.
3. Create a fresh temp XFS blob directory, master key and test clock per test; register hooks in the production order.

**Teardown procedure**:
1. Drop `t_<id>`, delete the blob directory, reset the clock offset to 0.
2. Run the leak check (blob-volume bytes and `accounts` row count equal to the post-setup values for tests that do not delete).

**Data isolation strategy**: one database per test from the template, one blob directory per test, accounts created through invite factories; no in-memory or fake stores for any query path. The only decorators are fault-injecting wrappers around real hooks (IT-07-9..11) and test `HealthContributor`s (IT-07-6); both are named in the test.

## Spec gaps found

- G-07-1: the late-registration method that gives `DeletionPurger` its hook list is not named in the description (only "registered late by 12"). Tests use the seam 12 uses; name needed from the contracts crate.
- G-07-2 (resolved 2026-11-07): concurrent `run_cycle` calls on one account are kept apart by a per-step `FOR UPDATE SKIP LOCKED` claim through the proposed `AccountAdmin::lock_deleting`; IT-07-12 states the rule, nothing is assumed.
- G-07-3: the HTTP code for `LastAdmin` is not in 11's error table (AT-07-2 step 1 accepts the 409/422 class); one fixed code is needed.
