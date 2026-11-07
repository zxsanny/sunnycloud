# Contract: AccountService and AccountDeletionService

**Component**: 07_admin
**Producer task**: E-17/c05 (`view`, `quota_view`, `request`), E-17/c04 (last-admin rule in `request`) — `_docs/02_tasks/todo/B05-05_account_view_deletion_request.md`, `B05-04_last_admin_guard.md` (renamed to their AZ keys at Step 4)
**Consumer tasks**: E-21/c12 (account handlers), E-22/c09 (`admin user delete`); in batch E-17/c06, E-17/c09
**Version**: 1.1.0
**Status**: draft (open points below)
**Last Updated**: 2026-11-07

## Purpose

A user's own account view and the request that starts account deletion. The request only flips the account to `deleting` and ends its access; the erasure is the `DeletionPurger`'s work (`deletion_purger.md`).

## Shape

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `AccountService::view` | `(&Principal) -> AccountView{id, display_name, role, identities, quota, root_node_id}` | `Unexpected` | async; caller's own account only |
| `AccountService::quota_view` | `(&Principal) -> QuotaView{limit_bytes?, effective_limit, used, reserved}` | `Unexpected` | async; equals `QuotaAdmin::view` of the caller |
| `AccountDeletionService::request` | `(&Principal, confirmation) -> DeletionAccepted{deadline}` | `Forbidden` (device-token principal), `ConfirmationMismatch`, `LastAdmin` | async; one short transaction |
| `AccountDeletionService::request_as_operator` (proposed `contracts` change, E-17) | `(account) -> DeletionAccepted{deadline}` | `LastAdmin` | async; one short transaction; no confirmation |

### HTTP resources owned by 07 (routed by 11)

| Method | Path | Auth | Body | Success |
|--------|------|------|------|---------|
| GET | `/api/v1/accounts/me` | SessionOrDevice | — | 200 `AccountView` |
| DELETE | `/api/v1/accounts/me` | SessionOnly | `{confirmation}` | 202 `{deadline}` |
| GET | `/api/v1/accounts/me/quota` | SessionOrDevice | — | 200 `QuotaView` |

## Invariants

- `request` refuses a device-token principal with `Forbidden` and a confirmation that does not match with `ConfirmationMismatch`; neither changes anything.
- The confirmation (decided 2026-11-07) is the caller's own account id as lowercase hyphenated UUID text, compared for exact equality with the principal's account id rendered that way: no trimming, no case folding. Empty, whitespace, wrong case and another account's id are all `ConfirmationMismatch`. The value is inferred from ST-07-2's cases; no earlier text names it.
- `request_as_operator` (decided 2026-11-07) serves `admin user delete --yes` (`Actor::Operator`, no principal): it skips the confirmation, because `--yes` is the confirmation, runs the same transaction as `request` including `lock_active_admins` and the last-admin guard (`LastAdmin`), and refuses nothing else. `request` is unchanged and stays session-only.
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

1. Resolved 2026-11-07: the confirmation value and its comparison (see Invariants).
2. Resolved 2026-11-07: the operator path is `request_as_operator`; adding it to `contracts` part 1 is a proposed `contracts` change task in E-17 (module-layout Rule 6), a dependency of E-17/c05 (AZ-3819).
3. The 01 port that supplies `display_name` and linked `identities` is not named.
4. `ConfirmationMismatch` has no code or status in the shared status table.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.1.0 | 2026-11-07 | Confirmation value and comparison fixed; `request_as_operator` added (proposed; frozen when its `contracts` change task is accepted) (AZ-3814) | spike AZ-3814 |
