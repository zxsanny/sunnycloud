# Contract: AccountService and AccountDeletionService

**Component**: 07_admin
**Producer task**: E-17/c05 (`view`, `quota_view`, `request`), E-17/c04 (last-admin rule in `request`) — `_docs/02_tasks/todo/B05-05_account_view_deletion_request.md`, `B05-04_last_admin_guard.md` (renamed to their AZ keys at Step 4)
**Consumer tasks**: E-21/c12 (account handlers), E-22/c09 (`admin user delete`); in batch E-17/c06, E-17/c09
**Version**: 1.0.0
**Status**: draft (open points below)
**Last Updated**: 2026-11-04

## Purpose

A user's own account view and the request that starts account deletion. The request only flips the account to `deleting` and ends its access; the erasure is the `DeletionPurger`'s work (`deletion_purger.md`).

## Shape

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `AccountService::view` | `(&Principal) -> AccountView{id, display_name, role, identities, quota, root_node_id}` | `Unexpected` | async; caller's own account only |
| `AccountService::quota_view` | `(&Principal) -> QuotaView{limit_bytes?, effective_limit, used, reserved}` | `Unexpected` | async; equals `QuotaAdmin::view` of the caller |
| `AccountDeletionService::request` | `(&Principal, confirmation) -> DeletionAccepted{deadline}` | `Forbidden` (device-token principal), `ConfirmationMismatch`, `LastAdmin` | async; one short transaction |

### HTTP resources owned by 07 (routed by 11)

| Method | Path | Auth | Body | Success |
|--------|------|------|------|---------|
| GET | `/api/v1/accounts/me` | SessionOrDevice | — | 200 `AccountView` |
| DELETE | `/api/v1/accounts/me` | SessionOnly | `{confirmation}` | 202 `{deadline}` |
| GET | `/api/v1/accounts/me/quota` | SessionOrDevice | — | 200 `QuotaView` |

## Invariants

- `request` refuses a device-token principal with `Forbidden` and a confirmation that does not match with `ConfirmationMismatch`; neither changes anything.
- `request` runs one short transaction: `lock_active_admins`, the last-admin guard, then `AccountAdmin::begin_deletion` (state `deleting`; 01 revokes sessions, device tokens and grants). Nothing else happens in the request: no erase hook, no blob work.
- `deadline` = now (injected clock) + `account_delete_deadline` (15 min by default).
- From commit on: every credential of the account is rejected (within 5 s, AC-21), commits are refused by `AccountGuard`, and the account's data still exists until the purger removes it.
- The audit entry for the request carries ids only.
- `view` and `quota_view` never return another account's values.

## Non-Goals

- Not covered: erasure, finalization and the deletion deadline alert (`deletion_purger.md`).
- Not covered: a recent-sign-in requirement (not added, 07 §8).

## Versioning Rules

- **Breaking changes** (a refusal removed, the request doing more work, a field of `AccountView` removed) require a new major version and notice to E-21 and E-22.
- **Non-breaking additions** (a new optional view field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| own-view | accounts A (two identities) and B | A's view has both identities and A's root node | IT-07-7 |
| device-refused | device principal, right confirmation | `Forbidden`; state active | ST-07-2 step 1 |
| bad-confirmation | empty, whitespace, wrong case, other account's id | `ConfirmationMismatch` | ST-07-2 steps 2-3 |
| accepted | session, right confirmation | deadline now + setting; `deleting`; credentials rejected; data present | IT-07-8 |
| last-admin | only active admin requests deletion | `LastAdmin` | IT-07-3 |

## Open points

1. The exact confirmation value and its comparison are not specified (E-17/c01).
2. The operator path (`admin user delete --yes`) has no principal; how it reaches `request` is open (contracts part 1 open point 5, E-17/c01).
3. The 01 port that supplies `display_name` and linked `identities` is not named.
4. `ConfirmationMismatch` has no code or status in the shared status table.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
