# Contract: MediaAdmin, media DoctorCheck and HealthContributor, metrics

**Component**: 08_media_jobs
**Producer task**: E-18/c11 — `_docs/02_tasks/todo/B05-22_media_admin_health_doctor.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-22/c13 (`admin media-requeue`, `admin media-stats`), E-22/c14 (`doctor`), E-22/c03 (registration; the contributor reaches 07's health page through the `HealthContributor` trait); in batch E-18/c12; pipeline AZ-3661
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The operator and health surface of 08: requeueing dead jobs, queue counts, the worker presence check that `doctor` and the admin health page show, and the exported metrics. Signatures are in `contracts` parts 1 and 3.

## Shape

| Port and method | Behaviour | Errors |
|-----------------|-----------|--------|
| `MediaAdmin::requeue_dead(filter{account?, kind?})` | set matching `dead` jobs to `queued` with attempts reset; return the count | `Unexpected` |
| `MediaAdmin::stats()` | `MediaStats`: job counts per lane and status | `Unexpected` |
| `DoctorCheck::run(ctx)` | findings from `worker_status` (table below) | none (problems are findings) |
| `HealthContributor::check()` | one `HealthItem`: not ok exactly when the doctor level is Warn or Fail; detail names the cause | none |

### Worker-status levels

| Condition | Level |
|-----------|-------|
| no `worker_status` row seen within three `worker_status_interval`, no queued or dead jobs | Warn ("no worker") |
| no row seen within three intervals, jobs queued or dead | Fail |
| `decoder_ok = false` | Fail |
| `image_age_days` above 14 | Warn |
| fresh row, `decoder_ok = true`, age at most 14 | Ok |

`seen_at` is always the API clock at the time of the report.

### Metrics

`jobs_queued`, `jobs_running` (gauges), `job_leases_expired_total`, `jobs_dead_total` (counters), `job_duration_seconds{kind}`, `derived_bytes_total{kind}`.

## Invariants

- The doctor check and the health item read the table only and never contact a supervisor.
- No finding, item or metric carries a secret or an account name.

## Non-Goals

- Not covered: the commands and the doctor aggregation (12), the health aggregation (07).

## Versioning Rules

- **Breaking changes** (a level changed, a metric renamed) require a new major version and notice to E-17 and E-22.
- **Non-breaking additions** (a new finding or metric) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| levels | IT-08-20 worker-status cases | Warn, Fail, Ok, Warn, Fail as listed | IT-08-20 |
| requeue | 4 dead jobs, filter by account then kind | counts; queued; attempts reset | IT-08-20 |
| stats | mixed jobs | equal to a direct count | IT-08-20 |
| health | each level | not ok on Warn or Fail | IT-07-6 |

## Open points

1. `worker_status_interval` is listed as a worker-only key in the helper key table, but the API needs it for the three-interval rule.
2. Thresholds for dead jobs and queue depth in the health item are not stated.
3. The `DoctorTag`s of this check are not stated.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
