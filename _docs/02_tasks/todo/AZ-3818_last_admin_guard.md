# Last-admin guard on every route and command

**Task**: AZ-3818_last_admin_guard
**Name**: Last-admin guard
**Description**: Demotion, disabling and account deletion run in one short transaction that locks the active admins, refuses with `LastAdmin` when no active admin would remain, and then applies the change, for the admin route, self-deletion and the operator commands alike
**Complexity**: 5 points
**Dependencies**: AZ-3817_admin_user_management, AZ-3819_account_view_deletion_request, AZ-3690_account_admin_and_guard
**Component**: 07 admin
**Tracker**: AZ-3818
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/account_admin_and_guard.md` — `lock_active_admins`, `set_role`, `set_state`, `begin_deletion` (E-15/c10).

## Problem

Every installation must keep at least one active admin (AC-23, decision 15). Three paths can remove one: role demotion, disabling, and deletion of the admin's own account; two admins can also demote each other at the same moment. A check outside the transaction, or one per path, lets a race leave zero admins.

## Outcome

- One guard shared by `update_user` (role and state), `AccountDeletionService::request`, and the operator path `AccountDeletionService::request_as_operator` (decided 2026-11-07, AZ-3814; a proposed `contracts` change task in E-17, CC-2).
- Inside one short transaction: `lock_active_admins`, then the guard, then the mutating `AccountAdmin` call.
- Disabled and deleting accounts never count as active admins.
- Two concurrent removals can never both pass; a lock conflict is retried at most once inside the port and never surfaced as a deadlock.

## Scope

### Included
- The guard and its use in the three mutation paths, for `Actor::Admin` and `Actor::Operator`.
- The concurrency suite IT-07-3 to IT-07-5 and ST-07-3.

### Excluded
- The HTTP status and the CLI exit code of a refusal (E-21/c12, E-22/c08, E-22/c09; system scenario FT-N-46).
- Sole-admin reset (G-12-3, E-15/c01).

## Acceptance Criteria

**AC-1: The only admin cannot be removed (AC-23)**
Given one active admin A, two ordinary users and one disabled admin D
When A is demoted, disabled, and A's deletion is requested, each as `Actor::Admin(A)` and as `Actor::Operator`
Then all six calls return `LastAdmin`, A's role, state and sessions are unchanged, and after another user is promoted to admin the same demotion of A succeeds

**AC-2: Concurrent demotions**
Given 100 rounds, each with exactly two active admins A and B
When A demotes B and B demotes A at the same moment on two connections released by a barrier
Then in every round exactly one call succeeds and the other returns `LastAdmin`, one active admin remains, and no deadlock error reaches the caller

**AC-3: Mixed races**
Given 200 rounds with admins A and B
When random pairs of {request deletion of A, request deletion of B, disable B, demote A, demote B} run concurrently
Then after every round at least one account has role admin and state active, every refused call returns `LastAdmin`, and an account in `deleting` is never counted as active

**AC-4: Delay between guard and write**
Given the AC-2 rounds with a 200 ms delay injected after `lock_active_admins` returns
When the two transactions run
Then the second waits on the lock and exactly one passes in every round

## Non-Functional Requirements

**Reliability**
- The guard transaction is short: no I/O and no wait other than the row locks.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Guard decision per action (demote, disable, delete) and active-admin count | Refuses only when no active admin would remain |
| AC-2 | Retry rule on a lock conflict | At most one retry, then the typed result |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | One active admin, one disabled admin | IT-07-3 | Six `LastAdmin`; demotion allowed after a promotion | FT-N-46, FT-P-65 |
| AC-2 | Two admins, barrier, 100 rounds | IT-07-4 | Exactly one success per round | |
| AC-3 | Two admins, 200 rounds | IT-07-5 | At least one active admin always | |
| AC-4 | 200 ms delay decorator around `AccountAdmin` | ST-07-3 | Second waits; one passes | |

## Constraints

- The lock order is fixed: active admin rows first, then the target row (inside `begin_deletion` or the setters), so the two paths cannot deadlock each other.
- 01 applies the change; 07 owns the rule (01 has no `LastAdmin`).

## Risks & Mitigation

**Risk 1: Lock waits under load**
- *Risk*: many admin actions queue behind the active-admin lock.
- *Mitigation*: admin actions are rare; the transaction holds no I/O.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-12-3: `LastAdmin` does not apply to the sole-admin reset (owner decision 2026-11-11, AZ-3681); the reset path does not use this guard | E-15 / E-22 owners | resolved | Low |

## Contract

This task implements the last-admin rule of `_docs/02_document/contracts/07_admin/admin_service.md` and `_docs/02_document/contracts/07_admin/account_services.md`.
Consumers MUST read those files — not this task spec — to discover the interface.
