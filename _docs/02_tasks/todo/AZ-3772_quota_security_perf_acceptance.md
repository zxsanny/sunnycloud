# Quota security, performance and acceptance tests

**Task**: AZ-3772_quota_security_perf_acceptance
**Name**: Quota security, perf and acceptance tests
**Description**: ST-07a-1..3 and PT-07a-1 at the ledger boundary, and the two stack-level acceptance tests AT-07a-1 (refusal leaves nothing reserved) and AT-07a-2 (derived data does not count)
**Complexity**: 2 points
**Dependencies**: AZ-3750_quota_ledger, AZ-3751_quota_admin_lifecycle, AZ-3752_quota_recompute, AZ-3753_quota_concurrency_suite, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3807_doctor_aggregation, AZ-3809_doctor_root_checks, AZ-3828_identity_admin_handlers, AZ-3829_tree_transfer_feed_handlers, AZ-3830_share_media_handlers, AZ-3831_internal_router_test_clock, AZ-3734_transfer_start_upload, AZ-3736_transfer_resume_abort, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3841_job_planner_enqueue_effect, AZ-3842_derived_store_accept_input, AZ-3843_worker_auth_jobs_facade, AZ-3872_worker_slot_loop, AZ-3857_decoder_image_executor, AZ-3860_decoder_video_executor; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9); for AT-07a-1 and AT-07a-2 on the compose stack:
**Component**: 07a quota
**Tracker**: AZ-3772
**Epic**: AZ-3631 (E-11)
**Ref**: E-11/c06

## Problem

The ledger guards money-like data (limit enforcement is a critical path): it must not leak across accounts, must survive hostile numbers and must not be alterable by the runtime role. Its hot-row throughput must cover AC-40's commit rate. Two user-visible promises can only be seen through the whole stack: a refused upload leaves nothing reserved, and thumbnails and transcodes never count against the quota. The epic row is 1 point; the stack setup for the two acceptance tests makes it 2.

## Outcome

- ST-07a-1 (account scoping), ST-07a-2 (overflow and extreme inputs), ST-07a-3 (role privileges) and PT-07a-1 (hot-row throughput) at the port boundary against real PostgreSQL 18.
- AT-07a-1 and AT-07a-2 against the compose test stack through the public API and the operator CLI.

## Scope

### Included
- The four component tests and the two acceptance tests, their data (DS-26 style session opener, DS-03 photos, DS-07 clip).

### Excluded
- System scenarios NFT-RES-05, NFT-SEC-27, NFT-SEC-46, NFT-PERF-01 (E-31).

## Acceptance Criteria

**AC-1: Account scoping (ST-07a-1)**
Given accounts A and B with distinct usage
When every `QuotaLedger` and `QuotaAdmin` method runs for A, and each runs once with a random UUID
Then B's row is unchanged byte for byte, the random id gives `NotFound` from `view` and `set_limit` and no effect from release and erase, and no log line holds another account's values

**AC-2: Extreme inputs (ST-07a-2)**
Given an account with limit 1 GiB and another whose limit the harness set to `i64::MAX`
When `reserve(i64::MAX)`, `reserve(-1)`, `adjust_used(i64::MIN)` and two reservations summing past `i64::MAX` run on each
Then each returns a typed error, the row stays valid with its CHECK intact, and there is no panic, no wrapped value and no raw database message

**AC-3: Role privileges (ST-07a-3)**
Given the runtime role `sunnycloud`
When it runs `ALTER TABLE quota DROP CONSTRAINT ...` and `TRUNCATE quota`
Then both are refused with permission denied

**AC-4: Hot-row throughput (PT-07a-1)**
Given 64 transactions in flight on one account, then 64 accounts with one each, closed loop of reserve + release + adjust for 2 minutes after a 10 s ramp on REF-SRV
When the run finishes
Then hot-account p95 is at most 30 ms (failure 100 ms), 64-account p95 at most 5 ms (failure 20 ms), hot-account throughput at least 500 tx/s (failure 200 tx/s), and the error rate excluding retried deadlocks is 0%

**AC-5: A refusal leaves nothing reserved (AT-07a-1)**
Given the compose stack and an account with limit 200 GiB
When 100 upload sessions of 50 GiB are opened, the account's own quota is read, and all sessions are aborted
Then at most the number allowed by the quota and the per-account session cap succeed and the rest get HTTP 507 `quota_exceeded`, `reserved` equals the sum of the successful sessions with `used` 0, and after the aborts `reserved` is 0

**AC-6: Derived data does not count (AT-07a-2)**
Given the compose stack with a worker, and 20 DS-03 photos and the DS-07 clip uploaded to one account
When all media jobs finish (polled through thumbnail states), and then the harness adds 777 to `used_bytes` and `sunnycloud doctor --json` runs
Then `used` equals the sum of the original sizes before and after the jobs, byte for byte, and doctor reports a quota drift finding for the account

## Non-Functional Requirements

**Performance**
- As AC-4; database connections 64, database host CPU under 60% on REF-SRV.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (test task) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Accounts A and B | ST-07a-1 | 0 changed columns on B | AC-59 |
| AC-2 | Limit 1 GiB and `i64::MAX` | ST-07a-2 | Typed errors; row valid | AC-39 |
| AC-3 | Runtime role | ST-07a-3 | Both refused | NFT-SEC-46 |
| AC-4 | One hot account; 64 accounts | PT-07a-1 | p95 and throughput within targets | NFT-PERF-01 |
| AC-5 | Compose stack, limit 200 GiB | AT-07a-1 | 507 for the rest; reserved matches; 0 after abort | NFT-RES-05 |
| AC-6 | Compose stack with worker, DS-03, DS-07 | AT-07a-2 | `used` unchanged by jobs; drift reported | AC-74 counting rule |

## Constraints

- PT-07a-1 timing on REF-SRV only; elsewhere `NOT-RUN(HW-REF)`, reported and never counted as a pass.
- AT tests use only the public API, the worker stack and the operator CLI; no component is faked or bypassed.

## Risks & Mitigation

**Risk 1: Long dependency chain for a small test task**
- *Risk*: AT-07a-1 and AT-07a-2 wait for transfer, media, HTTP and CLI work in other epics.
- *Mitigation*: the component tests (AC-1 to AC-4) can be built first; the acceptance tests are added when the stack runs.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AT-07a-1 and AT-07a-2 run through the full stack and tie this task to E-14, E-18 to E-22 and E-31; the owner may prefer to run them in E-31 instead | Owner | open | Low |
| AT-07a-1's expected count depends on the per-account session cap default, still an owner gap (G-R9, `tests/spec-gaps.md`) | Owner | open | Medium |
| REF-SRV not provisioned (R03); PT-07a-1 stays `NOT-RUN(HW-REF)` until it exists | Owner (hardware) | open | Medium |
