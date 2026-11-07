# QuotaAdmin::recompute with repair and drift logging

**Task**: AZ-3752_quota_recompute
**Name**: Quota drift check
**Description**: Compare stored usage and reservations with caller-supplied truth, report the drift, repair it on request, and complete the `QuotaAdmin` port
**Complexity**: 2 points
**Dependencies**: AZ-3751_quota_admin_lifecycle
**Component**: 07a quota
**Tracker**: AZ-3752
**Epic**: AZ-3631 (E-11)
**Ref**: E-11/c04
**Document dependencies**: `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` (port signatures, E-00/c03)

## Problem

Clamped releases, replacements and conflict copies can let stored usage drift from the real sum of live revisions (R11). `doctor` must report it and the operator must be able to repair it, but 07a may not read 02's or 03's tables (that would recreate the dependency cycle), so the caller supplies the truth.

## Outcome

- `recompute(account, live_bytes, reserved_bytes, repair)` runs in system scope, compares stored `used_bytes` with `live_bytes` and stored `reserved_bytes` with `reserved_bytes`, and returns `Drift{stored, computed}` for both columns.
- With `repair = true` it writes the supplied values; with `false` it changes nothing.
- Each drift found logs one WARN with ids only.
- With this method the `QuotaAdmin` port is complete.

## Scope

### Included
- `recompute`, repair, WARN log, completion of the `QuotaAdmin` port.

### Excluded
- Computing `live_bytes` (02 `LiveUsageSource`) and `reserved_bytes` (03 `ReservationSource`); the doctor aggregation and exit codes (E-22/c14).

## Acceptance Criteria

**AC-1: Drift found without change**
Given an account whose `used_bytes` the harness raised by 777 and whose `reserved_bytes` it lowered by 5
When `recompute(account, live_bytes, reserved_bytes, repair = false)` runs with the true values
Then it returns `Drift{stored: live + 777, computed: live}` and reports the reserved difference too, the row is unchanged, and one WARN line with ids only is logged

**AC-2: Repair and recheck**
Given the same account
When `recompute` runs with `repair = true` and then again with `repair = false`
Then the first sets the row to the supplied values and the second reports no drift

**AC-74: Usage is checked against the live sum (component part, R7)**
Given an account and the caller-supplied sum of current live revisions and open reservations
When `recompute` runs without repair
Then any difference in either column is reported as drift and nothing is changed, and equal values report no drift; the 0-drift outcome after 1,000 routed operations is checked end to end by E-22/p07

## Non-Functional Requirements

**Reliability**
- Never called on a request path: a doctor and test tool only.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Drift computation for both columns | Stored and computed reported separately |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | +777 used, -5 reserved | IT-07a-14 first call | Drift reported; row unchanged; one WARN | |
| AC-2 | Same account | IT-07a-14 repair and recheck | Repaired; no drift after | |
| AC-74 | Equal and unequal supplied values | `recompute` without repair | Drift only when different | NFT-RES-38 |

## Constraints

- System scope (operator path); 07a imports no tree or transfer type; the caller passes the numbers.

## Risks & Mitigation

**Risk 1: Repair with stale truth**
- *Risk*: the caller computes `live_bytes` while commits continue and repair writes an old value.
- *Mitigation*: repair only on explicit operator request (`doctor --repair` in E-22); the default reports only.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | - | - | - |

## Contract

This task produces/implements the `recompute` part of the contract at `_docs/02_document/contracts/07a_quota/quota-admin.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
