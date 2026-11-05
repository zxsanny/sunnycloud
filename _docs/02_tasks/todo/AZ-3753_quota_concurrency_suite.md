# Quota concurrency suite: AC-74 workload, interleaved accounts, savepoints

**Task**: AZ-3753_quota_concurrency_suite
**Name**: Quota concurrency suite
**Description**: Prove at the ledger boundary that 1,000 concurrent commit- and delete-shaped transactions leave usage exact and never above the quota, that accounts do not interfere, and that rolled-back entries leave no trace
**Complexity**: 3 points
**Dependencies**: AZ-3750_quota_ledger, AZ-3752_quota_recompute
**Component**: 07a quota
**Tracker**: AZ-3753
**Epic**: AZ-3631 (E-11)
**Ref**: E-11/c05

## Problem

AC-74 requires that after 1,000 concurrent commits and deletes the stored usage equals the sum of current live revisions and never exceeds the quota. The ledger's share of that guarantee (atomic, commutative updates that roll back with the caller) must be shown under real concurrency before 02 and 03 build on it.

## Outcome

- A seeded workload of 1,000 transactions in 20 waves of 50 on one account, modelling 02 and 03: commit (`reserve`, then `release_reservation` and `adjust_used(+size)`) or delete (`adjust_used(-size)` of an earlier commit), with one deadlock retry like 02, and a harness ledger of every success.
- The same workload with a second account receiving a fixed sequence, and with a share of the commit-shaped transactions rolled back to a savepoint after their ledger calls (as 02 does when an entry fails).

## Scope

### Included
- The three workload variants, the 5 ms sampler, final checks including `recompute`.

### Excluded
- The routed end-to-end version (E-22/p07, NFT-RES-38).

## Acceptance Criteria

**AC-74: Usage stays exact under 1,000 concurrent commits and deletes (component part, R7)**
Given one account with limit 200 MiB and 1,000 seeded transactions (sizes 1 KiB to 4 MiB) in 20 waves of 50, each retried once on deadlock
When the workload runs with a sampler reading the row every 5 ms
Then the final `used_bytes` equals the harness ledger sum, `reserved_bytes` is 0, no sample shows `used_bytes` or `used_bytes + reserved_bytes` above 200 MiB, there are 0 CHECK violations, `recompute(account, harness sum, 0, false)` reports stored equal to computed, and no `recompute` statement ran during the workload

**AC-1: Accounts do not interfere**
Given the AC-74 workload on account A and, at the same time, 100 rounds of reserve 1 MiB, adjust +1 MiB, release 1 MiB on account B
When both finish
Then B ends with used 100 MiB and reserved 0, and no lock wait on B's row exceeds 100 ms

**AC-2: Rolled-back entries leave no trace**
Given the AC-74 workload where a seeded share of the commit-shaped transactions roll back to a savepoint after their ledger calls and then commit the rest
When the workload finishes
Then none of the rolled-back deltas is in `used_bytes` or `reserved_bytes`, and the final values still equal the harness ledger

## Non-Functional Requirements

**Reliability**
- 0 database errors surfaced other than the retried deadlock.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (integration suite) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-74 | Limit 200 MiB, sizes-seeded (seed 20261103) | IT-07a-10 | Ledger exact; never above limit; no recompute during run | NFT-RES-38 |
| AC-1 | A under load, B fixed sequence | IT-07a-11 | B exact; lock waits <= 100 ms | |
| AC-2 | Savepoint share in the workload | IT-07a-9 under the IT-07a-10 workload | Rolled-back deltas absent | AC-68 |

## Constraints

- Real PostgreSQL 18, one database per test, separate pool connections per task (no interleaving on one connection).

## Risks & Mitigation

**Risk 1: Flaky timing**
- *Risk*: the lock-wait bound fails on a slow runner.
- *Mitigation*: the bound is read from `pg_locks` age, not wall time of the task; seeds are fixed.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | - | - | - |
