# Quota lifecycle hook and admin views and limits

**Task**: AZ-3751_quota_admin_lifecycle
**Name**: Quota lifecycle hook and admin views
**Description**: Create the quota row at account creation and delete it at erase, and give admins and users the quota view, a batched view for the admin list and per-user limits
**Complexity**: 3 points
**Dependencies**: AZ-3749_quota_skeleton, AZ-3750_quota_ledger; notes: the limit cases check refusal through `reserve`
**Component**: 07a quota
**Tracker**: AZ-3751
**Epic**: AZ-3631 (E-11)
**Ref**: E-11/c03
**Document dependencies**: `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` (port signatures, E-00/c03)

## Problem

Every account needs its quota row from the moment it exists (created in one transaction with the account, `data_model.md` §4) and none after it is erased (AC-27). The admin tool lists 500 accounts with usage (AC-1), users see their own quota, and admins set per-user limits, including limits below current use.

## Outcome

- `AccountLifecycleHook::initialize(tx, account)` inserts the row (limit unset, used 0, reserved 0) in the caller's transaction; `erase_batch(account, limit)` deletes it and returns `remaining = 0`, also on repeat and for unknown accounts.
- `QuotaAdmin::view` returns `QuotaView{limit_bytes?, effective_limit, used, reserved}` or `NotFound`; `views` answers for many ids in one round trip and omits unknown ids.
- `QuotaAdmin::set_limit(account, Option<i64>)` accepts 0 to 2^53 bytes (0 = no new data) or `None` for the installation default, refuses other values with `InvalidLimit` and unknown accounts with `NotFound`, never deletes data, and logs each change at INFO with ids only.

## Scope

### Included
- The hook's two methods and `view`, `views`, `set_limit`.

### Excluded
- `recompute` (E-11/c04). The `QuotaAdmin` port is complete only after E-11/c04; until then these methods are reached through the crate's own tests and no port method is a placeholder.
- Who may call `set_limit` (07 with `Scope::Admin`, the operator CLI) and the HTTP views (11).

## Acceptance Criteria

**AC-1: Row created with the account**
Given a new account inside a transaction that commits, and a second whose transaction rolls back
When `initialize` runs in each
Then the first account has limit unset, used 0 and reserved 0, `view` reports the installation default as `effective_limit`, and the second has no row

**AC-2: Limits, including below current use**
Given used 600 MiB
When `set_limit(Some(500 MiB))`, `reserve(1)`, `set_limit(None)`, `set_limit(Some(0))`, `set_limit(Some(-1))`, `set_limit(Some(2^53 + 1))` and `set_limit` for an unknown id run
Then 500 MiB is accepted with used still 600 MiB and the reserve refused, `None` restores the default as `effective_limit`, 0 is accepted, -1 and 2^53 + 1 return `InvalidLimit`, the unknown id returns `NotFound`, and each accepted change logs one INFO line with ids only

**AC-3: Batched views**
Given 500 accounts with seeded limits and usage (DS-29)
When `views` is called with all 500 ids, then with 3 unknown ids mixed in, and `view` with an unknown id
Then the map holds 500 entries equal to the seeded values in one database statement, the unknown ids are omitted without error, and `view` returns `NotFound`

**AC-4: Erase removes the row**
Given an account with 5 GiB reserved
When `erase_batch(account, 50)` runs twice, and once for an account never initialized
Then each returns `remaining = 0` without error, the row is gone and other accounts are untouched

## Non-Functional Requirements

**Performance**
- `views` for 500 accounts is one statement (AC-1 scale).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Limit bounds 0 and 2^53 inclusive | Accepted; outside refused |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Committed and rolled-back creation | IT-07a-1 | Row present; no row | AC-27 |
| AC-2 | Used 600 MiB | IT-07a-6 (bounds per G-07a-1) | Accepted, refused, default restored | AC-39 |
| AC-3 | DS-29 accounts-500 | IT-07a-12 (`pg_stat_statements`) | 500 entries, one statement, unknown omitted (G-07a-2) | NFT-PERF-01 |
| AC-4 | 5 GiB reserved | IT-07a-13 | `remaining = 0`, idempotent | FT-P-19, NFT-SEC-27 |

## Constraints

- Tests seed accounts through `initialize` inside a transaction (the real path), never by direct insert.
- Every statement is scoped by `account_id`; `views` runs under the caller's scope.

## Risks & Mitigation

**Risk 1: Lowered limit surprises a user**
- *Risk*: a limit below use blocks uploads with no deletion.
- *Mitigation*: allowed by design; the view shows used above the limit and new reservations are refused with `quota_exceeded`.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open (G-07a-1 and G-07a-2 resolved at the Step 5 gap pass) | - | - | - |

## Contract

This task produces/implements the view, views, set_limit and lifecycle-hook parts of the contract at `_docs/02_document/contracts/07a_quota/quota-admin.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
