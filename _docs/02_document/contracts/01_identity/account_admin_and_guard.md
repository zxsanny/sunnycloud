# Contract: account administration and the account guard

**Component**: 01_identity
**Producer task**: B04-10_account_admin_and_guard (E-15/c10)
**Consumer tasks**: E-17/c03, E-17/c04, E-17/c05, E-17/c06 (07 `AdminService`, last-admin guard, `AccountService` and `AccountDeletionService`, `DeletionPurger`, AZ-3820); E-13/c04, E-13/c05 (02 commit and node mutations call the guard)
**Version**: 1.3.0
**Status**: draft (open points in Non-Goals)
**Last Updated**: 2026-11-14

## Purpose

The account operations 07 combines with its own policy checks in one transaction, and the guard 02 calls first in every commit and tree mutation. Trait signatures live in `contracts` (E-00/c03).

## Shape

| Name | Signature (behavioural) | Throws / Errors | Blocking? |
|------|-----------|-----------------|-----------|
| `AccountAdmin::list_accounts` | `(filter, paging) -> [AccountInfo{id, display_name, role, state, last_login?, deletion_requested_at?}]` | none | async, own short transaction |
| `AccountAdmin::profile` | `(AccountId) -> AccountProfile{display_name, identities: Vec<IdentityView{provider, email?}>}` | `Unexpected` (also an unknown account) | async, own short read-only transaction in the account's scope |
| `AccountAdmin::lock_active_admins` | `(&mut DbTx) -> Vec<AccountId>` | `Unexpected` (DB errors; a second lost lock race) | async, row locks for update on active admins, ordered by id |
| `AccountAdmin::set_role` | `(&mut DbTx, AccountId, Role) -> ()` | typed (variants defined in `contracts`) | async, caller's transaction |
| `AccountAdmin::set_state` | `(&mut DbTx, AccountId, active \| disabled) -> ()` | typed | async, caller's transaction |
| `AccountAdmin::begin_deletion` | `(&mut DbTx, AccountId) -> ()` | typed | async, row lock for update |
| `AccountAdmin::finalize_deletion` | `(&mut DbTx, AccountId) -> ()` | typed | async, caller's transaction |
| `AccountAdmin::lock_deleting` | `(&mut DbTx, AccountId) -> bool` | `Unexpected` (DB errors) | async, caller's transaction; row lock `FOR NO KEY UPDATE SKIP LOCKED`, never waits |
| `AccountGuard::ensure_active` | `(&mut DbTx, AccountId) -> ()` | `AccountNotActive` | async, shared row lock |

`Role` is `user` or `admin` (data model §4.1). Account `state` is `active`, `disabled` or `deleting`.

## Invariants

- Methods that take `&mut DbTx` never open, commit or roll back a transaction; the caller's rollback undoes every change, including revocations.
- `set_state(disabled)` revokes the account's sessions and device tokens in the same transaction; after commit `authenticate` fails within `session_cache_ttl` + 50 ms.
- `begin_deletion` takes the account row for update, sets `deleting` and `deletion_requested_at`, and revokes sessions, device tokens and grants. From commit on, `authenticate` fails and `ensure_active` returns `AccountNotActive`.
- `finalize_deletion` deletes `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row. Identity does not check that the erase hooks finished; 07 calls it only after every hook reports `remaining = 0`.
- `ensure_active` takes a shared lock on the account row, so a commit already holding it finishes before `begin_deletion` proceeds, and any later commit is refused.
- `profile` reads only the named account: its display name and its identities, oldest first, in the account's scope (row-level security shows no other account's identities). It returns no token, no provider subject and no email that was not stored under the email rule. It changes nothing. 07 calls it with the principal's own account id for `AccountService::view`.
- `lock_deleting` is the deletion purger's per-step claim (CC-1): `SELECT ... FOR NO KEY UPDATE SKIP LOCKED` on the account row with state `deleting`, held until the caller's transaction ends. It returns `false` without waiting when another transaction holds the row, and `false` for an account in another state or one that is gone. The lock mode conflicts with another claim, `begin_deletion`, `set_state` and `finalize_deletion` (row locks for update) and with the guard's shared lock, but not with `FOR KEY SHARE`: an erase hook on its own connection can insert a row referencing the account while the purger holds the claim (owner decision 2026-11-14; IT-07-9 proves no self-deadlock).
- `list_accounts` returns `deletion_requested_at` (the first request time) for a `deleting` account and `None` otherwise.
- Identity has no last-admin rule and no `LastAdmin` error; 07 applies it after `lock_active_admins`.
- `lock_active_admins` locks the rows with role admin and state active (disabled and `deleting` accounts are never locked or returned), in id order, inside a savepoint. A lost lock race (SQLSTATE `40P01` deadlock, `55P03` lock not available, `40001` serialization failure) rolls back to the savepoint, releasing what the attempt took, and is retried once; a second one is `Unexpected`. The caller's transaction stays usable either way.

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
| profile | account with a Google (email) and a GitHub (no email) identity; a second account | name and both identities oldest first; the second account's identity absent; unknown account `Unexpected` | `01_identity/tests/account_profile.rs` |
| lock-retry | an admin row held by another transaction, `lock_timeout` 100 ms | `Unexpected` after two attempts (>= 200 ms); the transaction still usable | `01_identity/tests/account_guard.rs` |
| lock-deleting | active, `deleting` and unknown account; a second claimer; an insert of a row referencing the held account on another connection | only `deleting` is claimed; the second claim is `false` at once; the insert does not wait | `01_identity/tests/account_lock_deleting.rs` |
| guard-vs-deletion | guard holder, then `begin_deletion`, then new guard call | deletion waits; later guard `AccountNotActive` | IT-01-16 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
| 1.1.0 | 2026-11-14 | `profile(account)`: display name and linked identities for 07's account view (owner decision 2026-11-14; additive) | AZ-3819 |
| 1.2.0 | 2026-11-14 | `lock_active_admins`: one retry of a lost lock race inside a savepoint (owner decision 2026-11-14, AZ-3818); no signature change | AZ-3818 |
| 1.3.0 | 2026-11-14 | `lock_deleting(tx, account) -> bool`, the purger's claim with `FOR NO KEY UPDATE SKIP LOCKED` (CC-1, owner decision 2026-11-14); `AccountInfo.deletion_requested_at` in the listing. Additive | AZ-3820 |
