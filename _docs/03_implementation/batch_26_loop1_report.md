# Batch Report

**Batch**: 26
**Tasks**: AZ-3743_orphan_sweeper, AZ-3802_middleware_stack, AZ-3838_media_policy, AZ-3869_worker_api_client
**Date**: 2026-11-07

Four workers on four disjoint crates (`04_blobstore`, `11_http_api`, `media-shared`, `worker`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: one owner-approved allowlist line; the worker client's TLS setup (see Issues, item 1). Deferred: AZ-3790 (server crate, left for a later batch), AZ-3803, AZ-3804, AZ-3806, AZ-3812 (share `11_http_api` with AZ-3802), AZ-3745 (shares `04_blobstore` with AZ-3743), AZ-3893 (Windows only).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3743_orphan_sweeper | Done | `sunnycloud-blobstore`: `OrphanSweeper` (`orphan_sweeper/`: scan, `orphan_repo.rs`, probe hook); `<id>.gc` files are never reported, counted or deleted | 10 unit + 18 integration (incl. 1 log test) | Items 2-4 |
| AZ-3802_middleware_stack | Done | `sunnycloud-http-api`: `middleware/` (panic catcher, client IP, request id, metrics and log, security headers), peer address in the accept loop, metric label sets reworked | 16 unit + 11 integration | Items 5-7 |
| AZ-3838_media_policy | Done | `media-shared::policy`: `is_compliant`, `decide`, `poster_offset_ms`, `supported_kinds_for`; `media_policy.md` 1.0.1 | 19 | Item 8 |
| AZ-3869_worker_api_client | Done | `sunnycloud-worker`: `WorkerApiClient` (`api/`: status mapping, hash-verified streaming input, output upload) | 31 (30 + 1 added by the parent) | Items 1, 9-11 |

## Code Review Verdict: PASS_WITH_WARNINGS

9 findings, none blocking. One fixed in this batch (the worker client panicked in every workspace build, see Issues, item 1). Open: a `.mov` with H.264 is judged an MP4 (the container check cannot tell them apart); a rowless `.gc` file left by a crash is never reclaimed; orphan delete mode re-checks the row and unlinks in two steps; panics are counted only in `http_panics_total` (a name no doc defines); the public metric label types of AZ-3801 changed; worker input bytes are written before the hash is verified and truncated on mismatch; the worker client has no request timeout; statuses 09 section 2 does not list become `ApiError::Protocol` with no defined reaction.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. The first full run had 27 failures, all in the new worker tests (one cause, Issues item 1); after the fix a full workspace run, `cargo deny` and the four scripts were green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2252 (workspace, default features, real PostgreSQL 18)
- Passed: 2243
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Approved this session: allowlist line `src/components/04_blobstore/src/orphan_sweeper/orphan_repo.rs` (two read-only `SELECT`s on `blobs`; the sweep sees every account's rows).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | In any workspace build, `identity` and `sync-core` switch on reqwest's `rustls-no-provider`, and reqwest then panics when it builds a client without a TLS config (27 worker tests failed in the full run, none when the crate ran alone; a combined production build would also have panicked). Fixed: the worker asks for the feature, builds an empty-root rustls config (ring) and accepts `http` URLs only, since `worker_api_url` is the plain-HTTP backend listener. The worker now has a direct `rustls` dependency (already locked) | AZ-3869 | Done; test for the refused `https` address added | clear |
| 2 | A rowless `<id>.gc` file (crash after the GC sweeper settled) is never reclaimed | AZ-3743, batch 25 item 2 | Open: the scrubber (AZ-3745) or a follow-up must decide | unclear |
| 3 | Orphan sweeper: no rate limit (no config key); mtime exactly equal to the age is kept; names that are not blob ids are counted and logged once per run without a name; the INFO summary has no count slot | AZ-3743 | Accepted as implemented | unclear |
| 4 | `description.md` says `run_cycle(mode)`, the spec says `run_cycle(now)` with the mode from config | AZ-3743 | Followed the spec; the description row is not updated | unclear |
| 5 | Panic counter `http_panics_total` (exported `sunnycloud_http_panics_total`) and the `method`, `status_class` labels are not in `deployment/observability.md`; panics are not in `http_requests_total` | AZ-3802 | Owner to confirm the name and whether panics belong in the 5xx rate | unclear |
| 6 | `trusted_proxies` is CIDR only; `X-Forwarded-For` is read from the right and the first entry that is not a trusted proxy is the client (the spoofed `1.1.1.1, 198.51.100.7` gives `198.51.100.7`) | AZ-3802 | The right-to-left rule is the worker's reading of "proxy-set" | unclear |
| 7 | The request id is always fresh; a client `X-Request-Id` is never read (cross-epic conflict stays open); PT-11-2 `server-info` p95 was not measured | AZ-3802 | As the task states | clear |
| 8 | Media policy assumptions the docs do not state: an unreported codec, profile, bitrate or colour tag is not compliant; a container list containing `mp4` counts as MP4 (a MOV is indistinguishable); only Baseline, Main and High match; HDR means PQ or HLG transfer; `Plan.audio` is `None` for a clip without audio; media types are lowercase IANA names. Recorded as open point 3 of `media_policy.md` | AZ-3838 | Fail-closed choices; needs the owner and IT-08-17 (real-probe run) | unclear |
| 9 | Worker client: no request timeout (no config key); a status a route does not document is `ApiError::Protocol`; a destination write failure is `ApiError::Destination`; the loop task (c06) decides the reaction to each | AZ-3869 | c06 | unclear |
| 10 | Worker input is written as it streams and truncated to 0 bytes on any error, because there is no second file to stage into; the decoder starts only after `fetch_input` returns | AZ-3869 | Accepted | clear |
| 11 | PT-09-3 (resident memory for a 1 GiB input) is c12's measurement and was not run | AZ-3869 | Not run | clear |
| 12 | Items open from batches 18-25 stay open (purger lock strength, Retry-After, contracts change tickets, sc_error rows for 10 port variants, serde on the contracts media model, `utoipa` and a validator, CI wiring of `openapi_diff`, AZ-3726 privileged rows, AZ-3837 AC-4) | earlier batches | | unclear |

## Commit

`[AZ-3743] [AZ-3802] [AZ-3838] [AZ-3869] Orphan sweeper, middleware stack, media policy, worker API client` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: AZ-3790 per-command config, AZ-3803 body limits and timeouts, AZ-3804 version gate and server info, AZ-3806 auth extraction, AZ-3812 content-header mapper (the four HTTP tasks share `11_http_api`, one per batch), AZ-3745 scrubber, plus tasks unlocked by the policy and the worker client. 274 tasks left.
