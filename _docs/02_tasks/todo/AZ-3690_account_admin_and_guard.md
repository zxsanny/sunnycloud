# AccountAdmin and AccountGuard

**Task**: AZ-3690_account_admin_and_guard
**Name**: Account administration ports and the commit guard
**Description**: Give 07 the account operations it combines with its own checks in one transaction (roles, state, deletion), and give 02 the guard that lets an in-flight commit finish before deletion starts and refuses every later one
**Complexity**: 3 points
**Dependencies**: AZ-3683_authenticator_scopes_cache, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3768_contracts_part1, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base
**Component**: 01 identity
**Tracker**: AZ-3690
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/authenticator.md` — a disabled or deleting account must fail `authenticate` within its bound (produced by E-15/c03).

## Problem

07 decides admin policy (last-admin rule, deletion orchestration) but the account rows belong to identity. 07 must be able to lock, change and delete them inside its own transaction. A commit that races an account deletion must either finish before the deletion starts or be refused (seam for AC-68 and AC-74).

## Outcome

- `AccountAdmin`: `list_accounts`, `lock_active_admins`, `set_role`, `set_state`, `begin_deletion`, `finalize_deletion`; every method that changes rows takes the caller's transaction.
- `AccountGuard::ensure_active(tx, account)` with a shared row lock.
- The behavioural contract `_docs/02_document/contracts/01_identity/account_admin_and_guard.md`, which E-17 and E-13 read.

## Scope

### Included
- `list_accounts(filter, paging)` in its own short transaction.
- `lock_active_admins(tx)`: lock the rows of active admins for update and return their ids.
- `set_role(tx, account, role)`.
- `set_state(tx, account, active | disabled)`: a disabled account's sessions and device tokens are revoked in the same transaction.
- `begin_deletion(tx, account)`: lock the account row for update, set state `deleting` and `deletion_requested_at`, revoke sessions (reason `account_deleted`), device tokens and grants.
- `finalize_deletion(tx, account)`: delete the account's `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row.
- `ensure_active(tx, account)`: read the account state with a shared lock; anything but active is `AccountNotActive`.
- The contract file.

### Excluded
- Policy: the last-admin rule, who may call what, deletion confirmation (07, E-17/c04, E-17/c05).
- Driving the erase hooks and deciding when to finalize (07 `DeletionPurger`, E-17/c06).
- Calling the guard inside commits (02, E-13/c04, E-13/c05).

## Acceptance Criteria

**AC-1: Disable revokes in the caller's transaction (AC-21)**
Given an account with 2 sessions and 2 device tokens, and `session_cache_ttl = 200 ms`
When `set_state(disabled)` runs in a caller transaction that is rolled back, and then in one that commits
Then after the rollback nothing changed; after the commit the account's sessions and device tokens are revoked, `authenticate` returns `Unauthenticated`, and `revalidate` of a warm principal turns false within 250 ms

**AC-2: Begin deletion ends access at once**
Given an account with 2 sessions, 2 device tokens and 1 open grant
When `begin_deletion` commits
Then the state is `deleting` with `deletion_requested_at` set, the sessions, tokens and grant are revoked, `authenticate` fails for each credential, and `ensure_active` returns `AccountNotActive`

**AC-3: Finalize leaves nothing of the account (AC-27, identity half)**
Given the account of AC-2 after its hooks reported nothing remaining
When `finalize_deletion` commits
Then `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and `accounts` hold 0 rows for its id

**AC-4: Active admins can be locked for the last-admin check**
Given two active admins and one disabled admin
When transaction 1 calls `lock_active_admins` and transaction 2 calls it before transaction 1 commits
Then transaction 1 gets the two active admin ids, and transaction 2 waits (visible in `pg_locks`) until transaction 1 commits

**AC-5: Role change and listing**
Given a member account
When `set_role(admin)` commits and `list_accounts` runs
Then the account is listed with role admin and its state

**AC-6: The guard serialises with deletion (IT-01-16)**
Given one active account
When connection 1 opens a transaction and calls `ensure_active`, connection 2 calls `begin_deletion`, connection 1 commits, and then connection 3 calls `ensure_active`
Then `begin_deletion` waits in `pg_locks` until connection 1 commits and then completes (wait above 0 and under 2 s), and connection 3 gets `AccountNotActive`

## Non-Functional Requirements

**Reliability**
- No method opens its own transaction when it takes the caller's; the caller's rollback undoes everything (AC-1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Guard decision per state | Only `active` passes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 2 sessions, 2 devices; TTL 200 ms | IT-01-15 disable; IT-01-2 `set_state` path | Rollback undoes; commit revokes; flip within 250 ms | NFT-SEC-08 |
| AC-2 | Sessions, devices, grant | IT-01-15 `begin_deletion` | Deleting; all revoked; guard refuses | FT-P-19, NFT-RES-30 |
| AC-3 | After hooks report 0 | IT-01-15 `finalize_deletion` | 0 rows in the six tables | NFT-SEC-27 |
| AC-4 | 2 active, 1 disabled admin | IT-01-15 `lock_active_admins` | Two ids; second caller waits | FT-N-46 |
| AC-5 | Member account | `set_role`, `list_accounts` | Listed as admin | |
| AC-6 | One active account, 3 connections | IT-01-16 | Wait then complete; later guard refused | NFT-RES-34, NFT-RES-38 |

## Constraints

- 07 decides policy; identity only applies the change. There is no `update_user` port (01 AccountAdmin).
- The account-deletion flow through the real server is the pipeline task AZ-3665 (E-22/p12).

## Risks & Mitigation

**Risk 1: Lock order between guard and deletion**
- *Risk*: a commit holding the shared lock blocks deletion for long.
- *Mitigation*: commits are short transactions; AC-6 bounds the wait.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `set_state(active)` on a `deleting` account is not described (allowed, refused, or never called by 07) | E-15 / E-17 owners | open | Low |
| The `list_accounts` filter and paging fields are not named in 01 | E-15 / E-17 owners | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/01_identity/account_admin_and_guard.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
