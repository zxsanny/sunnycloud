# JobQueue: enqueue, claim, lease, release, skip, fail, reap, purge, requeue

**Task**: AZ-3840_job_queue
**Name**: Postgres job queue
**Description**: The system-scope repository behind every job state change: idempotent enqueue, one-statement `SKIP LOCKED` claim by lane and priority, lease extension, release, skipped close, failure with backoff on the API clock, reaping, terminal purge and requeue of dead jobs
**Complexity**: 5 points
**Dependencies**: AZ-3837_media_shared_params_protocol, AZ-3839_media_tables_config, AZ-3765_sc_clock, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3786_sc_observability
**Component**: 08 media_jobs
**Tracker**: AZ-3840
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — job kinds, lanes and the state rules the routes expose (E-18/c02).
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `ClaimedJob`, `SkipOutcome`, `Disposition`, `FailResult` (E-00/c06).
- `_docs/02_document/contracts/shared_db/sc_db_access.md` — `run_system` (E-06/c01).

## Problem

The supervisor has no database login (S4-1), so 08 must own the queue SQL inside the API. Interactive photos must never wait behind a bulk import (AC-10); a job must never be held by two workers; a crashed worker must only cost a lease (AC-71); and a job with exhausted attempts must not be handed out again.

## Outcome

- `enqueue_in_tx(tx, JobSpec, priority)` returns `Enqueued` or `AlreadyExists` (insert that ignores a conflict on the unique key).
- `claim(lane, worker_id)` is one statement: it first marks `dead` any job whose lease expired and whose attempts are exhausted, then takes the next job of that lane by `priority, available_at` with `SKIP LOCKED`, sets `running`, `attempts + 1`, `available_at = now + job_lease` and `leased_by`, and returns `ClaimedJob` with `lease_ms`.
- Job-scoped calls (`extend_lease`, `release`, `complete_skipped`, `fail`) require the job to be `running`, leased by the caller and unexpired; otherwise `LeaseLost`.
- `fail` applies `job_retry_backoff` doubling per attempt on the API clock, stores at most 500 characters of the error, and returns `Dead` for `Permanent` or when attempts reach `job_attempts`, else `Requeued`.
- `release` returns the job to `queued` with `attempts - 1`; `complete_skipped` closes it `succeeded` with outcome `skipped_compliant` or `obsolete` and no derived row.
- `reap(now)`, `purge_terminal(before)` (batched) and `requeue_dead(filter)` for the janitor and the admin port; metrics `job_leases_expired_total` and `jobs_dead_total` move at those events.

## Scope

### Included
- The nine repository operations above, in system scope only, and their two counters.

### Excluded
- Writing `succeeded` with outcome `derived` (only E-18/c07 does).
- Planning (E-18/c06); worker authentication (E-18/c08); scheduling the reaper (E-18/c10, E-22/c06).

## Acceptance Criteria

**AC-1: Idempotent enqueue**
Given one job spec
When it is enqueued twice, and once more with another `params_version`
Then the first returns `Enqueued`, the second `AlreadyExists`, the third `Enqueued`, and two rows exist

**AC-2: Ordering and lanes (AC-10)**
Given 10 bulk `stills` jobs (earlier `available_at`), 1 interactive `stills` job (later), 3 `transcode` and 2 `poster` jobs
When the `image` lane is claimed repeatedly, then the `video` lane
Then the first `image` claim returns the interactive job, the next ones return bulk jobs in `available_at` order, `video` claims return only `poster` and `transcode` jobs, an empty lane returns none, and each claim sets `running`, attempts 1, the holder and `lease_ms` equal to `job_lease`

**AC-3: No double claim (AC-45)**
Given 500 `image` jobs
When 20 claimers loop concurrently until the lane is empty
Then 500 distinct jobs are returned with 0 duplicates, no claim takes longer than 200 ms, and every row is `running` with one holder

**AC-4: Lease expiry (AC-71)**
Given worker W1 holding job J
When the clock passes `job_lease` + 1 s, W2 claims, and W1 then extends, fails, skips and releases J
Then W2 gets J with attempt 2, every W1 call returns `LeaseLost`, W2's extension returns `Extended` with `lease_ms`, and a third worker that never held J also gets `LeaseLost`

**AC-5: Failure, backoff and dead**
Given jobs A, B, C and D, test backoff 200 ms and `job_attempts` 3
When A fails with `Retry` twice (claimed in between), B fails with `Retry` three times, C fails `Permanent` once, and D fails with a 2,000-character error
Then A is `Requeued` with `available_at` now + 200 ms and then + 400 ms and is not claimable before that, B is `Dead` at the third failure, C is `Dead` at the first, D's stored error is at most 500 characters, and `jobs_dead_total` rose by 2

**AC-6: Release and skipped closes (FT-N-49)**
Given claimed jobs J1, J2 and J3
When J1 is released, J2 is closed `skipped_compliant` and J3 is closed `obsolete`, and a non-holder repeats the closes
Then J1 is `queued` with attempts back to 0 and claimable at once, J2 and J3 are `succeeded` with those outcomes and no derived row, and the non-holder gets `LeaseLost`

**AC-7: An exhausted expired job is never handed out (G-08-1)**
Given a job claimed 3 times with its lease expiring each time and the reaper paused
When a fourth claim runs on its lane
Then that job is not returned and is `dead` afterwards

**AC-8: Reap, purge, requeue**
Given running jobs with expired leases and exhausted attempts, terminal jobs older and newer than `job_terminal_ttl`, and 4 dead jobs of 2 accounts and 2 kinds
When `reap`, `purge_terminal` and `requeue_dead` by account and by kind run
Then the expired exhausted jobs become `dead` and `job_leases_expired_total` rises, only the old terminal rows are deleted, and each requeue returns the count of matching dead jobs and sets them `queued` with attempts reset

## Non-Functional Requirements

**Performance**
- Claim p95 5 ms, p99 10 ms, 1,000 claims/s with 100,000 queued rows (PT-08-1 in E-18/c13).

**Reliability**
- Every time value comes from the API clock; the worker's clock is never used.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Backoff per attempt; dead rule; error truncation | Doubling; dead at limit or permanent; 500 chars |
| AC-4 | Holder check | Only running, own and unexpired passes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | One spec | IT-08-2 repository part | Enqueued, AlreadyExists, Enqueued | AC-71 |
| AC-2 | Mixed lanes and priorities | IT-08-4 | Interactive first; lanes isolated | FT-P-20, FT-P-21 |
| AC-3 | 500 jobs, 20 claimers | IT-08-5 | 0 duplicates; under 200 ms | |
| AC-4 | W1, W2, test clock | IT-08-6 | Attempt 2; `LeaseLost` for W1 | NFT-RES-23 |
| AC-5 | Jobs A to D | IT-08-7 | Backoff, dead, truncation, counter | FT-N-06, NFT-RES-19 |
| AC-6 | J1 to J3 | IT-08-8 | Queued; skipped; obsolete | FT-N-49, FT-P-25 |
| AC-7 | Exhausted job, reaper paused | IT-08-19 | Not handed out; dead | |
| AC-8 | Mixed terminal and dead jobs | IT-08-15 reaper part; IT-08-20 requeue part | As listed | |

## Runtime Completeness

- Named capability: the PostgreSQL job queue with `SKIP LOCKED` (ADR-003, ADR-009).
- Production code that must exist: the real claim and state-change statements against PostgreSQL 18.
- Unacceptable substitutes: an in-memory queue, a fake store in any query-correctness test, or a claim made of several statements without the skip-locked rule.

## Constraints

- System scope only (`run_system`); no account-scope path reaches `jobs`.
- `succeeded` with outcome `derived` is never written here.

## Risks & Mitigation

**Risk 1: Claim polling load**
- *Risk*: about 4 claim requests per second hit the table.
- *Mitigation*: index-only candidate scan (E-18/c04 AC-4); an empty claim is cheap.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open (G-08-1 resolved at the Step 5 gap pass) | - | - | - |
