# Device grant bound to the account that looked it up

**Task**: AZ-4267_device_grant_lookup_binding
**Name**: Device grant lookup binding
**Description**: A pending device grant is bound at lookup to the account that looked up its user code; only that account can approve or deny it.
**Complexity**: 2 points
**Dependencies**: AZ-3692
**Component**: 01 identity (migration owned with 01)
**Tracker**: AZ-4267
**Epic**: AZ-3646 (E-15)

## Problem

Found by AZ-3692 (ST-01-1) on 2026-11-13, severity High: `DeviceGrantService::decide` accepts any session that names a pending grant id, so account B holding the id of a grant that account A looked up can approve A's device into B's account (or deny it). Owner decision 2026-11-13: bind the grant at lookup (one new `device_grants` column, no wire change).

## Acceptance Criteria

**AC-1: Bound at lookup**
Given a pending grant
When account A calls `lookup_for_approval` with its user code
Then the grant records A as its approver account

**AC-2: Other accounts cannot decide**
Given the grant bound to A
When account B calls `decide` (approve or deny) with its id
Then the answer is `NotFound` (as for an unknown id), nothing changes and no audit event is written

**AC-3: The suite's test passes**
Given `cross_account::account_b_cannot_decide_a_pending_grant_that_a_looked_up`
When it runs without `#[ignore]`
Then it passes

**AC-4: The binding does not move**
Given the grant bound to A
When account B looks up the same user code
Then the binding stays with A and B gets the answer an unknown code gets

## Scope

Included: the migration (one nullable column), identity's lookup and decide, the tests.
Excluded: wire changes.
