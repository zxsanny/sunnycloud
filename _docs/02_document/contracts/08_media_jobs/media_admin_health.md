# Contract: MediaAdmin, media DoctorCheck and HealthContributor, metrics

**Component**: 08_media_jobs
**Producer task**: E-18/c11 — `_docs/02_tasks/todo/AZ-3846_media_admin_health_doctor.md`
**Consumer tasks**: E-22/c13 (`admin media-requeue`, `admin media-stats`), E-22/c14 (`doctor`), E-22/c03 (registration; the contributor reaches 07's health page through the `HealthContributor` trait); in batch E-18/c12; pipeline AZ-3661
**Version**: 1.1.0
**Status**: active (open points 1 to 3 hold proposals; point 1 needs an `sc_config` change by the E-01 owner)
**Last Updated**: 2026-11-14

## Purpose

The operator and health surface of 08: requeueing dead jobs, queue counts, the worker presence check that `doctor` and the admin health page show, and the exported metrics. Signatures are in `contracts` parts 1 and 3.

## Shape

Types of crate `media-api`, registered by 12:

| Type | Ports | Constructor |
|------|-------|-------------|
| `MediaAdminService` | `MediaAdmin`; `ScrapeProbe` of the metrics registry (sets the queue gauges) | `new(db, queue: Arc<JobQueue>, metrics: Arc<JobMetrics>)` |
| `MediaHealth` | `DoctorCheck` (name `media_worker`, tags none), `HealthContributor` (name `media`) | `new(db, clock, &MediaConfig)` |
| `JobMetrics` | the metric families below | `register(&Metrics)` |

| Port and method | Behaviour | Errors |
|-----------------|-----------|--------|
| `MediaAdmin::requeue_dead(filter{account?, kind?})` | set matching `dead` jobs to `queued` with attempts reset, lease and finish cleared, due now; the filter fields combine with AND, an empty filter matches every dead job; return the count | `Unexpected` |
| `MediaAdmin::stats()` | `MediaStats`: one `JobCount` per (lane, status) present in `jobs`, ordered by lane then status; a pair with no job is absent | `Unexpected` |
| `DoctorCheck::run(ctx)` | findings from `worker_status` (table below); `ctx` plays no part; a table that cannot be read is one `Fail` | none (problems are findings) |
| `HealthContributor::check()` | one `HealthItem`: `Ok` / `Degraded` / `Down` for the doctor level Ok / Warn / Fail; detail = the message of the worst finding, then `; queue depth <queued>; dead jobs <dead>`; a failed read is `Down` | none |

### Worker-status levels

A row is fresh when `seen_at >= now − 3 × worker_status_interval` (API clock). Facts read in one statement: fresh rows, fresh rows with `decoder_ok = false`, the largest `image_age_days` of the fresh rows, queued jobs, dead jobs.

| Condition | Level | Message (no worker id, no account) |
|-----------|-------|---------|
| no fresh row, no queued or dead jobs | Warn | "no media worker has reported in the last <n>s; 0 jobs queued, 0 dead" |
| no fresh row, jobs queued or dead | Fail | same message with the counts |
| a fresh row with `decoder_ok = false` | Fail | "<k> of <n> media workers report a failing decoder self-check" |
| a fresh row with `image_age_days` above 14 | Warn | "the decoder image is <d> days old (more than 14)" |
| fresh rows, every decoder ok, age at most 14 | Ok | "<n> media workers reporting; decoder ok; image <d> days old" |

Both the decoder and the age finding are reported when both apply; the level of the check is the worst finding. `seen_at` is always the API clock at the time of the report.

### Metrics

| Family | Type | Labels | Moves |
|--------|------|--------|-------|
| `jobs_queued`, `jobs_running` | gauge | none | set from the `stats` counts before each scrape (scrape probe); a failed read keeps the last values and logs ERROR |
| `job_leases_expired_total` | counter | none | a running job taken over by a claim, or closed dead by the claim or the reaper |
| `jobs_dead_total` | counter | none | a job becomes `dead` (permanent or last failure, claim of an exhausted job, reaper) |
| `job_duration_seconds` | histogram | `kind` = job kind (`stills`, `poster`, `transcode`) | seconds from enqueue (`created_at`) to the close, observed when a job becomes `succeeded` (skipped, or its last output accepted) or `dead` |
| `derived_bytes_total` | counter | `kind` = job kind | the plaintext size of every accepted output (not of an `AlreadyPresent` one) |

Counters start at 0 (`derived_bytes_total` for each kind), so they are in the first scrape; a histogram series appears with its first observation. `worker_auth_failures_total` (AZ-3843) is registered by the same `JobMetrics`.

## Invariants

- The doctor check and the health item read the table only and never contact a supervisor.
- No finding, item or metric carries a secret, a worker id or an account.
- The health item is not ok exactly when the doctor level is Warn or Fail; dead jobs and queue depth are reported in its detail but have no threshold of their own.

## Non-Goals

- Not covered: the commands and the doctor aggregation (12), the health aggregation (07).

## Versioning Rules

- **Breaking changes** (a level changed, a metric renamed or relabelled) require a new major version and notice to E-17 and E-22.
- **Non-breaking additions** (a new finding or metric) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| levels | no row without and with queued or dead jobs; fresh age 3, age 15, decoder failing; stale row without and with a queued job | Warn, Fail, Fail; Ok, Warn, Fail; Warn, Fail | IT-08-20, `admin_health::doctor_levels_…`, unit `worker_health::tests::the_level_table_of_the_contract` |
| health | each level | `Degraded` / `Down` on Warn / Fail, detail with the cause and the counts, no worker id or account | IT-07-6 part, same test |
| requeue | 4 dead jobs over 2 accounts and 2 kinds; account A, then kind `transcode` | 2, then 1; queued, attempts 0, no lease | IT-08-20, `admin_health::requeue_dead_…` |
| stats | mixed jobs | equal to a direct count per lane and status | same test |
| metrics | one stills job succeeding with two outputs, one poster job dead, one queued, one running | six families present; dead +1; `kind` labels; gauges 1 and 1 | `metrics_scrape::a_scrape_has_…` |

## Open points (proposals, AZ-3846)

1. `worker_status_interval`: `MediaConfig.worker_status_interval` reads the key from `Settings` and `validate` refuses zero. Proposal: the key's readers in the `sc_config` table become `AW` (E-01 owner); until then the API process holds the key's default (30 s), so a changed interval reaches the doctor only after that change.
2. Thresholds for dead jobs and queue depth in the health item: none. The item follows the doctor level only (AC-3: "not ok exactly when the doctor level is Warn or Fail"); dead jobs already raise "no worker" to Fail, and both counts are in the detail.
3. `DoctorTag`s of this check: none, as 04's retention check. A restore does not change worker presence (`Restore`), and no worker runs before the stack starts (`PreFlight`).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.1.0 | 2026-11-14 | As built: type names, messages, health mapping and detail, metric semantics (`kind` = job kind, duration from enqueue), proposals for open points 1 to 3 | AZ-3846 |
