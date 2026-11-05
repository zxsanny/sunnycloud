# Admin performance: 500-user listing and a large account deletion

**Task**: AZ-3824_admin_performance
**Name**: Admin performance suite
**Description**: Measure the admin user listing over 500 accounts and the full deletion of a 100,000-file account at production batch settings while other accounts keep committing
**Complexity**: 3 points
**Dependencies**: AZ-3817_admin_user_management, AZ-3825_admin_hook_chain_integration, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3828_identity_admin_handlers, AZ-3829_tree_transfer_feed_handlers
**Component**: 07 admin
**Tracker**: AZ-3824
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/07_admin/admin_service.md` — `list_users` (E-17/c03).
- `_docs/02_document/contracts/07_admin/deletion_purger.md` — purge behaviour (E-17/c06).

## Problem

AC-1 needs admin tooling for 500 users, and AC-27 needs a deletion finished within 15 minutes for a large account without hurting other users. Both depend on real data volume and the production batch settings.

## Outcome

- PT-07-1 and PT-07-2 results recorded against their targets; the PT-07-1 targets are replaced by the measured baseline at the first run.
- PT-07-2 runs on the reference hardware; elsewhere its timing rows are `NOT-RUN(HW-REF)`.

## Scope

### Included
- PT-07-1 at the port level; PT-07-2 on the running server with the scheduler and production settings, data generated through the API.

### Excluded
- The small-account deletion pipeline (AZ-3665); system NFT-PERF-01.

## Acceptance Criteria

**AC-1: Listing over 500 accounts (AC-1)**
Given the DS-29 snapshot of 500 accounts
When 4 admin callers request pages of 20 at 20 calls/s in total for 2 minutes
Then p95 is at most 30 ms (failure above 100 ms), p99 at most 60 ms (failure above 200 ms), the error rate is 0%, at most 4 database connections are active and each call makes 2 queries

**AC-2: Large account deleted in time (AC-27)**
Given an account with 100,000 files, 1,000 shares, 100 sessions and 5,000 derived rows, production settings (batch 5000, interval 5 s), and 20 other accounts each committing one small file per second
When the account's deletion is requested and the scheduler runs the purger
Then request to finalize takes at most 5 min (failure above 15 min), the longest purger transaction is at most 2 s (failure above 10 s), the other accounts' commit p95 stays within 20% of their baseline (failure at 2x), and the purger uses at most 2 database connections

## Non-Functional Requirements

**Performance**
- As in AC-1 and AC-2 (PT-07-1, PT-07-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: measurement task | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-29 accounts-500 | PT-07-1 | p95 30 ms, p99 60 ms, 0% errors | NFT-PERF-01 |
| AC-2 | tree-large (`gen_tree.py --count 100000`), background commits | PT-07-2 | Finalize within 5 min; longest tx 2 s | NFT-SEC-27 |

## Constraints

- PT-07-2 runs on C-REF hardware; elsewhere only the correctness of the end state is checked.

## Risks & Mitigation

**Risk 1: GC throughput dominates (R12)**
- *Risk*: the tree hook waits for blob files the GC has not removed.
- *Mitigation*: GC `Due::Now`; the measurement isolates hook time from GC time in its report.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| PT-07-1 targets are proposed (07 sets none) and are replaced by the first measured baseline | E-17 owner | accepted-risk | Low |
| The reference hardware for PT-07-2 is not yet named (performance gaps G1, G11) | Owner | open | Low |
| PT-07-2 generates data through the API, which ties this task to E-21 and E-22 | E-17 owner | open | Low |
