# Janitor scheduler: intervals, failure survival, metrics, `scheduler_alive`

**Task**: AZ-3792_janitor_scheduler
**Name**: Janitor scheduler
**Description**: Run every registered `Janitor` on its interval inside `serve`, survive failing and panicking cycles, never overlap a janitor with itself, publish janitor metrics and the `scheduler_alive` readiness item, and run the audit purge as a janitor
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3768_contracts_part1, AZ-3765_sc_clock, AZ-3784_sc_audit, AZ-3786_sc_observability, AZ-3742_test_harness_skeleton
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3792
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `Janitor`, `CycleStats`, `ReadinessCheck`.
- `_docs/02_document/contracts/shared_audit/sc_audit.md` — `AuditPurge::purge_older_than`.

## Problem

All periodic work runs inside the one API process (ADR-002, decision 16) without a leader. A loop that dies on one error stops expiry, GC and scrub silently; two overlapping cycles of the same janitor double the work; a held advisory lock would keep a session open across a cycle's I/O. Helpers carry no `contracts` port, so the audit retention purge needs a `Janitor` wrapper in 12 (module-layout open point 6).

## Outcome

- Each registered janitor runs every interval with the current time from the clock helper; cycles of one janitor never overlap.
- A failed cycle is logged at ERROR with the janitor's name, counted in `janitor_cycles_total{task, result=failed}`, and the loop continues; a panic is caught, logged, counted and survived; a successful cycle sets `janitor_last_success_timestamp_seconds{task}`.
- `scheduler_alive` `ReadinessCheck`: Ok while the scheduler runs, Failing once it has stopped.
- `stop` ends the loops: no cycle starts after it returns.
- No advisory lock and no leader; claims are the janitors' own (`SKIP LOCKED` or leases). The scheduler opens no database connection of its own.
- An audit-retention janitor that calls `AuditPurge::purge_older_than(now - audit_retention)`; `audit_retention = 0` keeps every row.

## Scope

### Included
- The scheduler engine, its metrics, its readiness item and the audit-retention janitor.

### Excluded
- What each owner's cycle does, and the drain-until-empty budgets (owners).
- Which janitors are registered and with which intervals (E-22/c03).
- One-shot CLI cycles `admin gc-run` and `scrub run` (E-22/c10, E-22/c12).

## Acceptance Criteria

**AC-1: Interval (IT-12-9)**
Given a counting janitor with a 200 ms interval
When the scheduler runs for 10 s
Then consecutive `run_cycle` calls start 200 ms ± 100 ms apart

**AC-2: Failures and panics survived (IT-12-9)**
Given one janitor that always errors, one that panics on its first cycle, and the counting janitor
When the scheduler runs
Then each error is logged at ERROR with the janitor's name and counted with `result=failed`, the panic is caught and logged, both loops continue, and the counting janitor keeps its interval

**AC-3: No overlap**
Given a janitor whose cycle takes three times its interval
When the scheduler runs for 10 cycles' worth of time
Then no two `run_cycle` calls of that janitor are in progress at once

**AC-4: No leader lock (IT-12-9)**
Given `serve` with janitors cycling against PostgreSQL
When `pg_locks` is sampled during cycles
Then no advisory lock is held by the API process across a cycle

**AC-5: Metrics and readiness**
Given janitors that succeed and one that fails
When `/metrics` and `/readyz` are read, then the scheduler is stopped and `/readyz` is read again
Then `janitor_cycles_total{task, result}` and `janitor_last_success_timestamp_seconds{task}` exist per janitor, `scheduler_alive` is Ok while running and Failing after stop, and no cycle starts after stop returns

**AC-6: Audit retention janitor**
Given audit rows older and younger than `audit_retention`, and separately `audit_retention = 0`
When the audit-retention janitor cycles
Then only rows older than the cutoff are removed, through the purge function, and with 0 no row is removed

## Non-Functional Requirements

**Performance**
- The scheduler adds no query of its own; idle-cycle queries, cycle p95 and CPU are measured as PT-12-2 in E-22/c17 (3 or fewer queries, 20 ms, 2%).

**Reliability**
- Coverage of the failure-survival path at least 90%.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Timing with a manual clock | Calls at each interval |
| AC-2 | Erroring and panicking test janitors | Loop survives; ERROR line and failure count each time |
| AC-3 | Slow test janitor | Never two calls at once |
| AC-5 | Readiness item before and after stop | Ok, then Failing |
| AC-6 | Cutoff computation with a manual clock | `now - audit_retention`; none for 0 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-3 | In-process scheduler with three test janitors, 200 ms (IT-12-9 in-process part) | Run 10 s | Interval held; failures survived; no overlap | AC-80 |
| AC-4 | `serve` on the stack | `pg_locks` sampling (IT-12-9) | No advisory lock across a cycle | AC-68 |
| AC-5 | `serve` on the stack | `/metrics`, `/readyz`, stop | Families present; Ok then Failing | |
| AC-6 | Audit rows around the cutoff | Retention cycle | Only old rows removed | |

The stack part of IT-12-9 (`admin gc-run` 20 times beside the scheduler's GC and `scrub run --full`) is covered by AZ-3663 (E-22/p11) and AZ-3667 (E-22/p14) AC-80.

## Constraints

- Loops are timers around `run_cycle`; they hold no logic (decision 16).
- Test janitors are used only to inject failures into the real scheduler.

## Risks & Mitigation

**Risk 1: Janitors share CPU and I/O with requests (R12)**
- *Risk*: a busy cycle slows requests.
- *Mitigation*: owners drain within a time budget; PT-12-2 thresholds in E-22/c17.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-12-7 reads `janitor_cycles_total{name}`, while `observability.md` names the label `task`; this task follows `observability.md` | E-22 owner | open | Low |
| When `scheduler_alive` should report Degraded or Failing besides "stopped" (for example a janitor that never succeeds) is not stated | E-22 owner | open | Low |
| Whether `stop` waits for a running cycle or cancels it is not stated | E-22 owner | open | Low |
