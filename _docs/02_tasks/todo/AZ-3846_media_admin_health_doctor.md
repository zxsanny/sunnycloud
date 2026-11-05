# MediaAdmin, media health and doctor checks, metrics

**Task**: AZ-3846_media_admin_health_doctor
**Name**: Media admin, health, doctor and metrics
**Description**: Let the operator requeue dead jobs and see queue counts, report worker presence, decoder probe and image age to `doctor` and the admin health page, and export the media metrics
**Complexity**: 2 points
**Dependencies**: AZ-3840_job_queue, AZ-3842_derived_store_accept_input, AZ-3843_worker_auth_jobs_facade, AZ-3786_sc_observability, AZ-3768_contracts_part1
**Component**: 08 media_jobs
**Tracker**: AZ-3846
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `DoctorCheck`, `HealthContributor` (E-00/c03).
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `MediaAdmin` (E-00/c05).
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — metrics registry (E-09/c01).

## Problem

The worker has no direct channel to the operator; its only heartbeat is the `worker_status` row. The operator must see whether a worker is alive, its decoder works and its image is fresh (AC-63), and must be able to requeue jobs that died before a policy fix.

## Outcome

- `MediaAdmin::requeue_dead(filter{account?, kind?})` returns the count requeued; `stats()` returns job counts per lane and status.
- `DoctorCheck` levels from `worker_status`: no row seen within three status intervals is Warn "no worker", or Fail when jobs are queued or dead; `decoder_ok = false` is Fail; `image_age_days` above 14 is Warn; otherwise Ok.
- A `HealthContributor` for the admin health page reporting the same worker findings, plus dead jobs and queue depth.
- Metrics exported: `jobs_queued`, `jobs_running`, `job_leases_expired_total`, `jobs_dead_total`, `job_duration_seconds{kind}`, `derived_bytes_total{kind}`.

## Scope

### Included
- The admin port, the doctor check, the health contributor and the metric registration and gauges.
- The contract file `contracts/08_media_jobs/media_admin_health.md`.

### Excluded
- The commands `admin media-requeue` and `admin media-stats` (E-22/c13); `doctor` aggregation (E-22/c14); health aggregation (E-17/c07).

## Acceptance Criteria

**AC-1: Doctor levels (AC-63)**
Given in turn: no `worker_status` row with and without queued or dead jobs, a fresh row with `decoder_ok = true` and age 3, age 15, `decoder_ok = false`, and a row older than three intervals with and without queued jobs
When the doctor check runs
Then the levels are Warn "no worker" and Fail, Ok, Warn, Fail, and Warn and Fail

**AC-2: Requeue and stats**
Given 4 dead jobs over 2 accounts and 2 kinds
When `requeue_dead` runs for account A, then for kind `transcode`, and `stats()` is read
Then each requeue returns the count of matching dead jobs and leaves them `queued` with attempts reset, and the stats equal a direct count per lane and status

**AC-3: Health contributor**
Given each AC-1 case
When the admin health page reads the media contributor
Then its item is not ok exactly when the doctor level is Warn or Fail, and its detail names the cause without secrets

**AC-4: Metrics exported**
Given one job that succeeds with outputs and one that dies
When the metrics endpoint is scraped
Then all six metrics are present, the dead counter rose by 1, the duration and derived-bytes metrics carry the job kind label, and the gauges equal the queued and running counts

## Non-Functional Requirements

**Reliability**
- The doctor check reads the table only; it never contacts the supervisor.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Level decision table | As listed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Worker status cases | IT-08-20 doctor part | Levels as listed | |
| AC-2 | 4 dead jobs | IT-08-20 requeue and stats | Counts; queued; stats equal | |
| AC-3 | Worker status cases | IT-07-6 with the real 08 contributor | Not ok on Warn or Fail | |
| AC-4 | One success, one dead | Metrics scrape | Six metrics; labels | |

## Constraints

- The `worker_status` row is the only worker heartbeat; there is no transport from the supervisor to the CLI.

## Risks & Mitigation

**Risk 1: Dead jobs for hostile files stay for 14 days**
- *Risk*: the operator does not notice them.
- *Mitigation*: `jobs_dead_total`, the health item and `admin media-requeue`.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The doctor needs `worker_status_interval` (three intervals), which the helper key table lists as a worker-only key | E-18 / E-01 owners | open | Low |
| No threshold is given for dead jobs or queue depth in the health item (IT-07-6 shows "dead jobs (down)" only as an example) | E-18 owner | open | Low |
| The `DoctorCheck` tags of this check are not stated | E-18 / E-22 owners | open | Low |

## Contract

This task produces `_docs/02_document/contracts/08_media_jobs/media_admin_health.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
