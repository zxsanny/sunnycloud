# Contract: AdminService (user administration, last-admin rule, health)

**Component**: 07_admin
**Producer task**: E-17/c02 (construction), E-17/c03 (`list_users`, `update_user`, `set_quota`), E-17/c04 (last-admin rule), E-17/c07 (`health`) — `_docs/02_tasks/todo/B05-02_admin_crate_skeleton.md`, `B05-03_admin_user_management.md`, `B05-04_last_admin_guard.md`, `B05-07_admin_health_aggregation.md` (renamed to their AZ keys at Step 4)
**Consumer tasks**: E-21/c12 (admin handlers), E-22/c03 (composition root), E-22/c08 (`admin promote|demote`), E-22/c09 (`admin user list|set-quota`); in batch E-17/c08, E-17/c09, E-17/c10
**Version**: 1.0.1
**Status**: draft (open points below)
**Last Updated**: 2026-11-07

## Purpose

The admin operations behind the web admin page and the operator CLI: list users with usage, change role and state, set quota limits, and read installation health. 07 owns the rule that the last active admin cannot be removed. Trait signatures are in `contracts` part 1 (E-00/c03); this file freezes the behaviour.

## Shape

### Operations

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `list_users` | `(Actor, page) -> Vec<AdminUserView{account, role, state, last_login, used, limit}>` | `Forbidden` | async; one `AccountAdmin::list_accounts` and one `QuotaAdmin::views` call per page; page size `admin_list_page_size` |
| `update_user` | `(Actor, account, role?, state?) -> ()` | `Forbidden`, `NotFound`, `LastAdmin` | async; one short transaction: `lock_active_admins`, guard, `set_role` and/or `set_state` |
| `set_quota` | `(Actor, account, limit: Option<i64>) -> ()` | `Forbidden`, `NotFound`, `InvalidLimit` (from 07a) | async; `QuotaAdmin::set_limit`; `None` = installation default |
| `health` | `(Actor) -> HealthReport{items: Vec<(name, HealthItem)>}` | `Forbidden` | async; each contributor check bounded by `health_check_timeout` |

`Actor` = `Admin(Principal)` or `Operator`. `Admin(p)` passes only when `p` is a session principal with role admin (`Scope::Admin`); a device-token principal never passes. `Operator` is the trusted CLI.

### Errors on the wire (mapped by 11)

| Error | Code | Status |
|-------|------|--------|
| `LastAdmin` | `last_admin` | 409 |
| `NotFound` | `not_found` | 404 |
| `Forbidden` | `forbidden` | 403 (G-S6, decided 2026-11-07); another account's id is `NotFound`, 404 |
| `InvalidLimit` | open (see Open points) | open |

### HTTP resources owned by 07 (routed by 11)

| Method | Path | Auth | Body | Success |
|--------|------|------|------|---------|
| GET | `/api/v1/admin/users` | Admin | — | 200 page of `AdminUserView` |
| PATCH | `/api/v1/admin/users/{id}` | Admin | `{role?, state?}` | 200 |
| PUT | `/api/v1/admin/users/{id}/quota` | Admin | `{limit_bytes}` or null | 200 |
| GET | `/api/v1/admin/health` | Admin | — | 200 `HealthReport` |

### Construction (composition root)

`AdminService` is built with `AccountAdmin`, `QuotaAdmin`, the `HealthContributor` list, the admin settings, the audit writer, the clock and the database handle, all passed to its constructor. There is no global registry.

## Invariants

- Last-admin rule: demotion, disabling and deletion of an account run in one short transaction that first calls `AccountAdmin::lock_active_admins`; if the change would leave no account with role admin and state active, the call returns `LastAdmin` and changes nothing. Disabled and `deleting` accounts are never active. The same rule serves `update_user`, `AccountDeletionService::request` and the operator path. Two concurrent removals never both pass; a lock conflict is retried at most once inside the port.
- Every successful mutation writes one `admin_action` audit row with actor, subject account id and action code, and no name or email.
- `list_users` returns every account exactly once across pages, in a stable order; no field carries a token or secret.
- 07 issues no SQL; accounts come from 01 and usage from 07a.
- `health` lists every registered contributor by `name()`; a check that does not answer within `health_check_timeout` is reported `down` with a timed-out detail and does not hide the others.

## Non-Goals

- Not covered: deleting another user's account (no such operation; G-AS-1 is an owner decision).
- Not covered: invites (01, routed under `/api/v1/admin/invites`), `/healthz` and `/readyz` (11).

## Versioning Rules

- **Breaking changes** (an error removed or changing meaning, the last-admin rule weakened, a field of `AdminUserView` removed) require a new major version and notice to E-21 and E-22.
- **Non-breaking additions** (a new optional field, a new contributor) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| list-paging | 45 accounts, page size 20 | pages 20, 20, 5; each account once | IT-07-1 |
| change-audit | role, state and two quota changes | 4 `admin_action` rows, ids only | IT-07-2 |
| last-admin | demote, disable, delete the only active admin | `LastAdmin` each; nothing changed | IT-07-3 |
| concurrent-demotion | two admins demote each other | exactly one passes | IT-07-4 |
| non-admin | user session, admin device token | `Forbidden` | ST-07-1 |
| health-timeout | contributor sleeping 5 s, timeout 300 ms | `down` within 500 ms; others present | IT-07-6 |

## Open points

1. `contracts` part 1 lists only `Unexpected` (or no error) for `list_users` and `health`; `Forbidden` on all four operations (`AdminError::Forbidden`) needs a minor `contracts` change, a proposed `contracts` change task in E-17 (E-17/c01 AC-4) and a dependency of E-17/c05 and E-17/c06.
2. Resolved 2026-11-07 (G-S6): a non-admin caller gets 403 `forbidden`; another account's id gets 404 `not_found`. ST-07-1, NFT-SEC-13 and E-31/c03 use this record.
3. `HealthReport` has items only; whether it carries an overall status is not stated.
4. `InvalidLimit` has no code in the shared status table; `health_check_timeout` and `admin_list_page_size` have no defaults.
5. G-AS-1: the web app lists "delete user", which 07 does not offer.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.0.1 | 2026-11-07 | `Forbidden` status fixed at 403 (G-S6) (AZ-3814) | spike AZ-3814 |
