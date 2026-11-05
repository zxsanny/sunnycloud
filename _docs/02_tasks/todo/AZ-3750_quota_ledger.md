# QuotaLedger: reserve, release and adjust in the caller's transaction

**Task**: AZ-3750_quota_ledger
**Name**: Quota ledger
**Description**: The three ledger calls that uploads and commits make inside their own transactions: a single conditional reserve that can never overshoot, a clamped release and a signed usage adjustment that can never go negative
**Complexity**: 3 points
**Dependencies**: AZ-3749_quota_skeleton, AZ-3786_sc_observability
**Component**: 07a quota
**Tracker**: AZ-3750
**Epic**: AZ-3631 (E-11)
**Ref**: E-11/c02
**Document dependencies**: `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` (port signatures, E-00/c03)

## Problem

100 upload sessions that each declare 50 GiB must not push an account past its quota (AC-39), and usage must stay exact under concurrent commits and deletes (AC-74). The ledger runs inside 03's and 02's transactions, so it must neither open its own transaction nor leave changes behind when the caller rolls back.

## Outcome

- `reserve(tx, account, bytes)` is one conditional update that adds `bytes` to `reserved_bytes` only when `used + reserved + bytes` stays within the effective limit (the account's limit or `default_quota`); otherwise it returns `QuotaExceeded{limit, used, reserved}` and changes nothing. A successful reserve holds the row lock until the caller's transaction ends.
- `release_reservation(tx, account, bytes)` subtracts and clamps at 0, logging one WARN (ids only) when it would go negative.
- `adjust_used(tx, account, delta)` applies a signed delta and returns `InvariantViolation` when the result would be negative, leaving the row unchanged.

## Scope

### Included
- The three methods, effective-limit fallback, the WARN log.

### Excluded
- Session cap and free-space floor (03, 04), admin views and limits (E-11/c03), drift checks (E-11/c04), concurrency suite (E-11/c05).

## Acceptance Criteria

**AC-1: Reserve within the limit**
Given limit 1 GiB and used 300 MiB
When `reserve(100 MiB)` runs twice
Then both return `Reserved`, `reserved_bytes` is 200 MiB and `used_bytes` stays 300 MiB

**AC-2: A refusal changes nothing**
Given limit 1 GiB, used 800 MiB and reserved 100 MiB
When `reserve(124 MiB)`, `reserve(123 MiB)` and `reserve(1)` run in turn
Then the first returns `QuotaExceeded{limit: 1 GiB, used: 800 MiB, reserved: 100 MiB}` with the row unchanged, the second returns `Reserved` (used + reserved equals the limit exactly), and the third returns `QuotaExceeded`

**AC-3: Concurrent reservations cannot overshoot**
Given limit 1 TiB, used 0, and a reader sampling the row every 5 ms
When 100 tasks each reserve 50 GiB in their own transaction
Then exactly 20 succeed and 80 get `QuotaExceeded`, every sample shows `reserved_bytes` at or below 1 TiB, the final value is 1000 GiB, and no database error other than a retried deadlock occurs

**AC-4: The reservation lock lasts to the end of the caller's transaction**
Given transaction A that reserves 1 GiB and waits 500 ms, and transaction B that reserves 1 GiB on the same account 100 ms later
When A rolls back (and, in a second run, commits)
Then B completes at least 400 ms after it started; after the rollback only B's reservation is counted, after the commit both are

**AC-5: Signed adjustment and invariant**
Given used 100 bytes
When `adjust_used(+50)`, `adjust_used(-120)` and `adjust_used(-31)` run
Then used becomes 150, then 30, and the third returns `InvariantViolation` with used still 30 and no constraint text in the error

**AC-6: Release clamps at zero**
Given reserved 100 MiB
When `release_reservation(40 MiB)` and then `release_reservation(200 MiB)` run
Then reserved becomes 60 MiB and then 0, neither call errors, and exactly one WARN line with the account id is logged

**AC-7: Commit-shaped calls roll back together**
Given reserved 10 MiB and used 0
When one transaction runs `release_reservation(10 MiB)` and `adjust_used(+10 MiB)` and then commits, rolls back to a savepoint, or aborts
Then the commit gives reserved 0 and used 10 MiB, and the savepoint rollback and the abort leave reserved 10 MiB and used 0; a concurrent reader never sees used + reserved above the limit

**AC-8: Effective limit falls back to the default**
Given an account with no limit and `default_quota = 10 GiB`
When reservations reach 10 GiB and one more byte is requested
Then the last one returns `QuotaExceeded` with limit 10 GiB

## Non-Functional Requirements

**Performance**
- Hot account p95 (reserve + release + adjust) at most 30 ms, failure 100 ms; at least 500 tx/s, failure 200 tx/s (PT-07a-1, E-11/c06).

**Reliability**
- Stateless: no cache and no in-memory state.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Reserve race with 100 tasks | Never above the limit |
| AC-6 | Clamp on release | 0, one WARN |
| AC-5 | Negative adjust refused | `InvariantViolation`, row unchanged |
| AC-8 | Effective-limit fallback | Default used when the limit is unset |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Limit 1 GiB, used 300 MiB | IT-07a-2 | `Reserved` twice; 200 MiB reserved | |
| AC-2 | Limit 1 GiB, used 800, reserved 100 MiB | IT-07a-3 | Refused unchanged; exact fit accepted | AC-39 |
| AC-3 | Limit 1 TiB, 100 x 50 GiB | IT-07a-4 | Exactly 20 reserved | NFT-RES-05, NFT-RES-LIM-01 |
| AC-4 | Two transactions | IT-07a-5 | B waits for A | |
| AC-5 | Used 100 | IT-07a-7 | 150, 30, `InvariantViolation` | AC-74 |
| AC-6 | Reserved 100 MiB | IT-07a-8 | 60 MiB, 0, one WARN | AC-68 |
| AC-7 | Reserved 10 MiB | IT-07a-9 | Commit applies; rollbacks undo | AC-68, AC-74 |

## Constraints

- Real PostgreSQL 18 only (the conditional update and row locks are the behaviour under test); concurrency cases use separate pool connections.
- The ledger never opens, commits or rolls back a transaction; every statement is scoped by `account_id`.

## Risks & Mitigation

**Risk 1: Hot row per account (R07)**
- *Risk*: one account's commits serialise on its quota row.
- *Mitigation*: updates are commutative and short; 02 aggregates deltas per batch; measured in PT-07a-1.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The outcome of `reserve` or `adjust_used` for an account without a quota row is not stated (ST-07a-1 covers only view, set_limit, release and erase) | Epic owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/07a_quota/quota-ledger.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
