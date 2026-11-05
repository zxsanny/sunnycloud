# Contract: account administration and the account guard

**Component**: 01_identity
**Producer task**: B04-10_account_admin_and_guard (E-15/c10)
**Consumer tasks**: E-17/c03, E-17/c04, E-17/c05, E-17/c06 (07 `AdminService`, last-admin guard, `AccountDeletionService`, `DeletionPurger`); E-13/c04, E-13/c05 (02 commit and node mutations call the guard)
**Version**: 1.0.0
**Status**: draft (open points in Non-Goals)
**Last Updated**: 2026-11-04

## Purpose

The account operations 07 combines with its own policy checks in one transaction, and the guard 02 calls first in every commit and tree mutation. Trait signatures live in `contracts` (E-00/c03).

## Shape

| Name | Signature (behavioural) | Throws / Errors | Blocking? |
|------|-----------|-----------------|-----------|
| `AccountAdmin::list_accounts` | `(filter, paging) -> [AccountInfo]` | none | async, own short transaction |
| `AccountAdmin::lock_active_admins` | `(&mut DbTx) -> Vec<AccountId>` | DB errors | async, row locks for update on active admins |
| `AccountAdmin::set_role` | `(&mut DbTx, AccountId, Role) -> ()` | typed (variants defined in `contracts`) | async, caller's transaction |
| `AccountAdmin::set_state` | `(&mut DbTx, AccountId, active \| disabled) -> ()` | typed | async, caller's transaction |
| `AccountAdmin::begin_deletion` | `(&mut DbTx, AccountId) -> ()` | typed | async, row lock for update |
| `AccountAdmin::finalize_deletion` | `(&mut DbTx, AccountId) -> ()` | typed | async, caller's transaction |
| `AccountGuard::ensure_active` | `(&mut DbTx, AccountId) -> ()` | `AccountNotActive` | async, shared row lock |

`Role` is `user` or `admin` (data model §4.1). Account `state` is `active`, `disabled` or `deleting`.

## Invariants

- Methods that take `&mut DbTx` never open, commit or roll back a transaction; the caller's rollback undoes every change, including revocations.
- `set_state(disabled)` revokes the account's sessions and device tokens in the same transaction; after commit `authenticate` fails within `session_cache_ttl` + 50 ms.
- `begin_deletion` takes the account row for update, sets `deleting` and `deletion_requested_at`, and revokes sessions, device tokens and grants. From commit on, `authenticate` fails and `ensure_active` returns `AccountNotActive`.
- `finalize_deletion` deletes `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row. Identity does not check that the erase hooks finished; 07 calls it only after every hook reports `remaining = 0`.
- `ensure_active` takes a shared lock on the account row, so a commit already holding it finishes before `begin_deletion` proceeds, and any later commit is refused.
- Identity has no last-admin rule and no `LastAdmin` error; 07 applies it after `lock_active_admins`.

## Non-Goals

- Not covered: admin policy and HTTP mapping (07, 11).
- Open: `set_state(active)` on a `deleting` account; the fields of the `list_accounts` filter and paging.

## Versioning Rules

- **Breaking changes** (a method that stops taking the caller's transaction, a new state value, finalize deleting fewer tables) require a new major version.
- **Non-breaking additions** (a new optional filter field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| disable-rollback | `set_state(disabled)` then caller rollback | nothing changed | IT-01-15 |
| disable-commit | `set_state(disabled)` committed | credentials fail within the cache bound | IT-01-15, IT-01-2 |
| finalize | after hooks report 0 | 0 rows in the six tables | IT-01-15 |
| guard-vs-deletion | guard holder, then `begin_deletion`, then new guard call | deletion waits; later guard `AccountNotActive` | IT-01-16 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
