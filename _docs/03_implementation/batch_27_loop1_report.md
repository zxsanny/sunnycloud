# Batch Report

**Batch**: 27
**Tasks**: AZ-3745_scrubber, AZ-3790_per_command_config, AZ-3806_auth_extraction, AZ-3876_worker_policy_step
**Date**: 2026-11-07

Four workers on four disjoint crates (`04_blobstore`, `server`, `11_http_api`, `worker`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: two owner-approved changes (one allowlist line; `load_keys` in the shared `sc_config` crate, added at the owner's choice after the AZ-3790 worker found the loader could not meet AC-1) and the silent-discard fix in `04_blobstore/src/activity.rs`. Deferred: AZ-3746 (shares `04_blobstore` with AZ-3745), AZ-3803, AZ-3804, AZ-3812 (share `11_http_api` with AZ-3806), AZ-3893 (Windows only), all decision spikes.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3745_scrubber | Done | `sunnycloud-blobstore`: `BlobScrubber` (the `Scrubber` port), `ScrubJanitor`, `ActivityGauge` (writer and reader count in-flight writes and open downloads), seven `scrub_*` metrics, system-scope `scrub_repo.rs` | 3 unit + 15 integration + 1 log | Items 1-5 |
| AZ-3790_per_command_config | Done as far as the commands exist | `sunnycloud-server`: `command_config.rs` (per-command key lists, `serve` validates every owner's section, `migrate` refuses without `db_migrator_url`), uses the new `sc_config::load_keys`; `operator_command_surface.md` 1.2.0 | 9 + 9 (real binary) | Items 6-8 |
| AZ-3806_auth_extraction | Done; cookie name and device prefix come from config | `sunnycloud-http-api`: `auth/` (per-route auth layer from the route's `AuthClass`, `Authenticated` extension, worker identity) | 17 + 1 (RSS bound) | Items 9-11 |
| AZ-3876_worker_policy_step | Done | `sunnycloud-worker`: `policy_step/` (`choose_step`, `check_output`, `OutputInvalid`) | 12 | Item 12 |
| (shared) `sc_config::load_keys` | Done | `sc_config`: loader that binds only the wanted keys | 6 | Item 6 |

## Code Review Verdict: PASS_WITH_WARNINGS

9 findings, none blocking; one fixed in this batch (an empty `Err(_) => {}` arm in the activity gauge, which `sc_error --test silent_catch` rejects). Open: cookie name and device prefix have no owner value and the OpenAPI document still names `__Host-session`; most commands are `Unrestricted` and `serve` does not validate the HTTP, database and vault sections; the scrub test takes about 2.5 minutes in a debug build against a 60 s limit; scrub run state is in memory; `status` finds the latest summary by paging the audit log; policy-step error dispositions are undefined; `OutputInvalid` shape and code spelling are unconfirmed; `load` and `load_keys` report several missing secrets differently.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. The full run had two failures (`sc_error --test silent_catch`, one cause, fixed); after the fix workspace clippy, `sunnycloud-blobstore` and `sc_error` (284 tests) and the four scripts were re-run green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2325 (workspace, default features, real PostgreSQL 18)
- Passed: 2316 (2314 in the full run + the 2 `silent_catch` tests re-run after the fix)
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Approved this session: allowlist line `src/components/04_blobstore/src/scrub/scrub_repo.rs` (the scrub selects, marks and counts blobs of every account); extending `sc_config` with `load_keys` so a command binds only the keys it uses.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | A scrub that meets a missing file looks for `<id>.gc` and the own name first; if the GC sweeper has either, it defers without reporting or deleting, otherwise re-reads the row in a new short transaction and marks only a `committed` row. Rowless `.gc` files cannot be reclaimed by the scrub (it walks rows, not directories); that belongs in the orphan sweeper | AZ-3745, batch 26 item 2 | Reclaim still open | unclear |
| 2 | `batch` size has no config key or documented value: it is a required constructor argument the composition root (AZ-3789) must choose | AZ-3745 | No default invented | unclear |
| 3 | "Scrub paused too long" WARN: no duration defined; one WARN is logged only if a pause lasts to the end of `scrub_cycle_budget` | AZ-3745 | Worker's reading | unclear |
| 4 | `latest_summary` pages the audit log from the epoch (SQL on `audit_log` is forbidden); the summary `duration_ms` unit is the worker's choice; `unavailable_at` is not cleared after an operator restore (existing open point) | AZ-3745 | Accepted | unclear |
| 5 | The 300-blob AC-1/AC-2 test takes about 2.5 minutes in debug (unoptimised ChaCha20), against the 60 s of IT-04-11; only an opt-level override in the workspace `Cargo.toml` would fix it | AZ-3745 | Open | unclear |
| 6 | `sc_config::load` reports every missing required key to every command and names only the first missing secret. Added `load_keys(sources, wanted)`: unwanted keys are not bound, unknown variables still refuse, several missing wanted secrets are listed in one refusal; `load` is unchanged | AZ-3790 AC-1 | Done | clear |
| 7 | `key-check`, `admin setup-token`, the other admin, scrub, media and doctor commands, `migrate revert` and `migrate status` are `Unrestricted` until their tasks (AZ-3789 and later) name their keys; only `version` can fully succeed today, the others get past configuration and exit 2 "not implemented" | AZ-3790 | Open point 9 of the contract | unclear |
| 8 | `serve` does not validate the HTTP (11), database and vault sections: none has a callable validation | AZ-3790 | Owners | unclear |
| 9 | Session cookie name and device-token prefix: no doc defines them, `sc_ids` issues no prefixed tokens, so they come from `HttpConfig.credentials`; whoever wires `HttpConfig` in 12 must supply both; the OpenAPI document's `__Host-session` disagrees with what auth reads | AZ-3806 | Owner decision | unclear |
| 10 | `DeviceOnly` is checked in 11 (`principal.kind == Device`) because 01 has no scope for it; a bearer wins over a cookie; worker routes pass the `Authorization` bearer to 08 as opaque text and never call 01 | AZ-3806 | Worker's reading | unclear |
| 11 | `retained_ok` is true only for a session principal with role Admin | AZ-3806 | Interpretation | clear |
| 12 | `OutputInvalid` is a unit struct with code `"OutputInvalid"` (other worker codes are snake_case); a poster probe without duration and an unknown `params_version` return errors with no documented disposition; a photo probe on a transcode job is not checked; the slot loop (AZ-3872) does not exist yet and must call the pure functions | AZ-3876 | AZ-3872 | unclear |
| 13 | Items open from batches 18-26 stay open (purger lock strength, Retry-After, contracts change tickets, sc_error rows for 10 port variants, serde on the contracts media model, `utoipa` and a validator, CI wiring of `openapi_diff`, `http_panics_total`, media policy open point 3, AZ-3726 privileged rows, AZ-3837 AC-4) | earlier batches | | unclear |

## Commit

`[AZ-3745] [AZ-3790] [AZ-3806] [AZ-3876] Scrubber, per-command config, auth extraction, worker policy step` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: AZ-3746 blob maintenance ports, one of AZ-3803/3804/3812 (the HTTP tasks share `11_http_api`), plus tasks unlocked by the scrubber, `load_keys`, auth and the policy step. 270 tasks left.
