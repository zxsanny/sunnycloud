# Batch Report

**Batch**: 60
**Tasks**: AZ-3737_transfer_stage_small (done), AZ-3845_media_janitors_erase_hook (done), AZ-3846_media_admin_health_doctor (done), AZ-3931_s12_verified_download_spike (done; Safari `NOT-RUN(HW-DEVICE)`, gate S12 to the owner)
**Date**: 2026-11-14

Three parallel workers: 03 (`stage_small`), 08 (AZ-3845 then AZ-3846, one worker because both write 08), and the S12 download spike (documents only; harness in the session scratchpad, not in the repository). The parent let the API read `worker_status_interval` (`sc_config` readers `W` -> `AW`, needed by the 08 doctor), aligned `deployment/observability.md` and the config helper doc with the as-built 08 metrics, verified the AZ-3737 account-guard decision against AZ-3734's record (owner, 2026-11-13), reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3737_transfer_stage_small | Done | `stage_small` in 03 (`stage.rs`) and `impl UploadService for PgUploadService` (`service.rs`). Order: guard + key lookup (identical row `Staged` with no body byte read, other `ClientKeyMismatch`) + staged cap; in-flight cap and quota pre-check; body read, length and BLAKE3; `write_complete` (04 floor before the file); one transaction (guard, `reserve`, staged cap again under the quota row lock, `blobs` uploading, completed session row); file discarded when it refuses. 0 B to 2 MiB, every refusal writes nothing (asserted), failed reservation discards the file. 13 integration + 1 unit test |
| AZ-3845_media_janitors_erase_hook | Done | `MediaLifecycle` (`AccountLifecycleHook`: jobs first, then derived rows, blobs queued `AccountDeleted` `Due::Now`; `BlobOwnerCleanup` inside the sweeper's transaction), `JobReaper`, `DerivedJanitor`. AC-1..AC-4 with the real 04 `GcQueue` and `GcSweeper` |
| AZ-3846_media_admin_health_doctor | Done | `MediaAdminService` (`requeue_dead`, `stats`, scrape probe for `jobs_queued` / `jobs_running`), `MediaHealth` (`DoctorCheck` `media_worker`, `HealthContributor` `media`; Warn -> Degraded, Fail -> Down; no ids in findings), `job_duration_seconds{kind}`, `derived_bytes_total{kind}`. AC-1..AC-4 |
| AZ-3931_s12_verified_download_spike | Done (Safari not run) | Firefox 159 (no automation): candidate A2 — ranged pull of 8 MiB, at most 2 in flight, each 2 MiB block hashed with the WASM `hash_block` into an OPFS file, saved only after the rollup matches — peaks at 123-155 MB for 1 and 2 GiB, hash equal, corrupt block detected with nothing saved, CSP of 16 §5 clean. Streamed fetch without ranges grows to ~2 GB on a fast link (no flow control); a Service Worker save is ended by Firefox after ~30 s and leaves a truncated file named as complete. Chromium and Playwright WebKit as extra evidence (WebKit OPFS quota 896 MiB). Safari `NOT-RUN(HW-DEVICE)`: gate S12 owner decision (a) measure on the owner Mac, (b) Safari unverified at every size, (c) a lower verified limit from Safari's OPFS quota. Recorded in 16 description §5 and §15, IT-16-8, spec-gaps IT-16-8 (partly resolved) |

## Parent changes

- `src/shared/config/src/table.rs`: `worker_status_interval` readers `AW` (API reads it for the doctor's three-interval window); `01_helper_config.md` notes it.
- `deployment/observability.md`: `jobs_queued`, `jobs_running` without labels; `job_duration_seconds` and `derived_bytes_total` labelled by job kind.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3404 passed, 0 failed, 21 ignored. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros in the tree and no migration in this batch.

Warnings (recorded, not fixed):
- 08 `DerivedJanitor` vs a concurrent commit of the same content: check-then-act; the new file can lose its thumbnail until a requeue (E-18 owner).
- 03 an expired, not yet reaped staged row still answers a repeat `stage_small` with `Staged` (reaper task E-14/c11).
- 08 the orphan scan is an anti-join over all `derived` rows each janitor cycle.
- 03 staged-cap refusals are not counted in `upload_rejections_total` (metrics task E-14/c16).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | Body above 2 MiB for `stage_small` | AZ-3737 flag (Low) | `LengthMismatch`, no body byte read (only answer both contract and IT-03-5 allow); proposal for the E-14 owner | unclear |
| 2 | Expired unreaped staged row answers `Staged` | AZ-3737 | Warning above; reaper task | unclear |
| 3 | `QuotaAdmin::view` added to 03's `UploadPorts` | AZ-3737 | 03 only; contracts untouched | clear |
| 4 | `worker_status_interval` readable only by the worker | AZ-3846 flag (Low) | Readers `AW` (parent) | clear |
| 5 | Dead-job / queue-depth thresholds for the health item | AZ-3846 flag (Low) | None of their own; the item follows the doctor level, counts in the detail; proposal | unclear |
| 6 | Doctor tags of `media_worker` | AZ-3846 flag (Low) | None (like 04's retention check); proposal | unclear |
| 7 | `observability.md` labelled `jobs_queued` by `thumbnail`/`transcode` | AZ-3846 | Aligned to the 08 contract (parent) | clear |
| 8 | Derived janitor vs same-content commit race | AZ-3845 | Warning above | unclear |
| 9 | Gate S12: Safari not measured | AZ-3931 | Owner decision (a)/(b)/(c); E-26/c12 builds the labelled unverified Safari path until then | unclear |
| 10 | When to delete the OPFS copy after save; quota check before start; ETag match across ranges; private windows | AZ-3931 | Open items for E-26/c12 | unclear |

## Commit

`[AZ-3737] [AZ-3845] [AZ-3846] [AZ-3931] Stage small, media upkeep, S12 spike` — Loop: 1

## Next Batch

Cumulative review of batches 58-60, then recompute the ready tasks.
