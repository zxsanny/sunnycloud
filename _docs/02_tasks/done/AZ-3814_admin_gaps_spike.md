# Admin gaps: purger claim, non-admin status, deletion inputs (spike)

**Task**: AZ-3814_admin_gaps_spike
**Name**: Admin decompose-time gaps
**Description**: Record the purger claim mechanism (G-07-2), the status for admin-only operations called by a non-admin (G-S6), the deletion confirmation value and the operator deletion entry, so the purger, deletion request and security tasks can be built without guessing
**Complexity**: 2 points
**Dependencies**: None
**Component**: 07 admin
**Tracker**: AZ-3814
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c01

## Problem

The gap pass fixed that concurrent purgers are kept apart by `FOR UPDATE SKIP LOCKED` on the `deleting` account row, but left open whether the claim is taken per step or kept as a lease column, and the claim must never span a cycle's idle time. The account row belongs to 01, and 07 owns no tables and has no port that claims it. G-S6 leaves open whether a non-admin calling an admin-only operation gets 403 or 404; ST-07-1 and NFT-SEC-13 need one value. Two inputs of the deletion request are also unspecified: the exact confirmation value that `request` accepts (ST-07-2 tests empty, whitespace, wrong case and another account's id), and how `admin user delete` reaches the request without a session principal (`contracts` part 1, open point 5).

## Outcome

- One written answer for each of the four questions, recorded in `components/07_admin/description.md` and `tests/spec-gaps.md` with the date.
- The answers unblock E-17/c05 (deletion request), E-17/c06 (purger) and E-17/c09 (security suite), and the matrix of E-21/c18.
- Any port change the answers need is named as a follow-up `contracts` change task before E-17/c05 and E-17/c06 start.

## Scope

### Included
- G-07-2: per-step claim or lease column; the port method that performs the claim and its owner (01 `AccountAdmin` or a new column in 01's table); proof that no claim is held across idle time.
- G-S6: one status for admin-only operations called by a non-admin principal.
- The confirmation value of `AccountDeletionService::request` and how it is compared.
- The operator path of account deletion (`admin user delete --yes`, `Actor::Operator`).

### Excluded
- Building any of it (E-17/c05, E-17/c06).
- Sole-admin reset versus `LastAdmin` (G-12-3: E-15/c01 and E-22/c01).

## Acceptance Criteria

**AC-1: Claim mechanism recorded (G-07-2)**
Given the gap-pass default and the short-transaction rule
When the spike ends
Then 07 §12 names the mechanism (per-step claim or lease), the port method that takes the claim and the epic that owns it, and states that no claim is held across a cycle's idle time; `tests/spec-gaps.md` shows G-07-2 resolved with the date; IT-07-12's expected result no longer says "assumed"

**AC-2: Non-admin status recorded (G-S6)**
Given ST-07-1, NFT-SEC-13 and the AC-59 matrix
When the spike ends
Then one status (403 or 404) for an admin-only operation called by a non-admin is written in 07 §4 and `tests/spec-gaps.md`, ST-07-1 and NFT-SEC-13 use it, and the record is the same one E-31/c03 uses for G-S6

**AC-3: Deletion inputs recorded**
Given ST-07-2 and `contracts` part 1 open point 5
When the spike ends
Then 07 §3 states the exact confirmation value `request` accepts and how it is compared (case, whitespace), and how the operator command reaches the deletion request without a principal; ST-07-2 steps 2 and 3 have one expected result each

**AC-4: Port changes named**
Given the answers of AC-1 to AC-3
When an answer changes a port signature in `contracts` (claim method, operator deletion entry, `Forbidden` on `AdminService`)
Then the spike lists one `contracts` change task in the providing epic (module-layout Rule 6) and E-17/c05 and E-17/c06 list it as a dependency

## Non-Functional Requirements

**Reliability**
- The chosen claim keeps the longest purger transaction within PT-07-2's bound (2 s target, 10 s failure).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: document outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 07 description, spec-gaps | Review of the recorded claim rule | Mechanism, port, owner and idle-time rule present | IT-07-12, PT-07-2 |
| AC-2 | ST-07-1, NFT-SEC-13 | Review of the expected values | One status used everywhere | AC-59 |
| AC-3 | ST-07-2, contracts part 1 | Review of the deletion inputs | Confirmation value and operator path written | AC-57 |
| AC-4 | The answers | Review of port impact | Contracts-change task listed or "none needed" | |

## Constraints

- Decisions are written into the existing documents; no new document.
- An owner decision is needed only if an answer changes an acceptance criterion.

## Risks & Mitigation

**Risk 1: A lease column changes 01's table**
- *Risk*: a lease column is a schema change in 01's migration domain.
- *Mitigation*: AC-4 names the owning task before E-17/c06 starts.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-S6 is also listed under E-31/c03; two tasks must record one answer | E-17 / E-31 owners | resolved 2026-11-07: 403, one record for both tasks | Low |

## Resolution (2026-11-07)

Owner-approved decisions:
- G-07-2: per-step claim, no lease column. Each purger step takes `SELECT ... FOR UPDATE SKIP LOCKED` on the `deleting` account row via the proposed `AccountAdmin::lock_deleting(tx, account) -> bool` (a `contracts` change in E-15, CC-1), in its own short transaction.
- KNOWN RISK, open: hooks run on their own connections; a hook inserting a row with a foreign key to `accounts` takes `FOR KEY SHARE` on that row, which `FOR UPDATE` blocks (self-deadlock); `FOR NO KEY UPDATE` avoids it. The owner has been asked; E-17/c06 must confirm no deadlock in IT-07-9.
- G-S6: a non-admin on an admin-only operation gets 403 `forbidden`; another account's id gets 404 (ST-07-1, NFT-SEC-13 step 4).
- Recorded in: `components/07_admin/description.md` (sections 4 and 12), `components/07_admin/tests.md`, `epics/E-17_admin.md`, `tests/security-tests.md`, and AZ-3665 and AZ-3820 (flagged concerns).
