# Batch Report

**Batch**: 90
**Tasks**: AZ-4334_serve_test_clock_wiring (new, owner decision 2026-11-18), AZ-3666_pipeline_db_restore_resync (two workers in parallel; parent: epoch doctor check, CI steps, harness fix)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4334 | Done | `server/Cargo.toml` (`[features]`), `server/src/wiring.rs`, `server/src/root/{mod,api}.rs`, `server/src/serve/http.rs`, `shared/config/{Cargo.toml,src/table.rs}`, `server/tests/{test_clock_serve.rs,serve_support/mod.rs,root_support/mod.rs}`, `http_server.md` 1.5.1, `common-helpers/01_helper_config.md`, CI (`rust-lint.sh` step 4/6, `rust-test` step) | feature `test_clock_serve` 2 (3 runs), default `test_clock_serve` 2 | Discovered #1 |
| AZ-3666 | Done | `server/tests/pipeline_db_restore.rs`, `pipeline_support/restore.rs`; production (parent): `server/src/epoch_check.rs` (new), `server/src/root/lists.rs`, `server/src/lib.rs`; CI `install-tools.sh pg-client-18` and `SC_PG_TOOLS` in `rust-test` | 8/8 (3 runs before the epoch check: 7 + 1 honest failure) | Discovered #2-#5 |

AZ-4334: feature `test-clock` now forwards to `sunnycloud-http-api/test-clock`, `sc_clock/test-clock` and `sc_config/test-clock`; `serve` builds one `OffsetClock`, registered as the context's `dyn Clock` and as the `OffsetClock` handle 11 serves on `:9091` (`root::register_with_test_clock`); key `test_clock_addr` (default `0.0.0.0:9091`) exists only in that build, so a release binary refuses it as an unknown key (exit 3). A mutation check (a second `SystemClock` swapped in) made AC-1 and AC-2 fail. The default build is unchanged (AC-3 tests in the default build). `root_support::context` registers the same clock under the feature (parent), so in-process composition tests also pass with `--features test-clock`.

AZ-3666: a real `pg_dump` / `pg_restore` 18 cycle (dump, then deletions, a GC run and an epoch bump, then restore into a recreated database). Owner decision 2026-11-18 on the tools: CI installs `postgresql-client-18=18.7-1.pgdg24.04+2` from PGDG with the signing-key fingerprint pinned (`install-tools.sh pg-client-18`); tests find the tools in `SC_PG_TOOLS` or on PATH, check the major version, and fail (never skip) when missing. Local runs here use a scratchpad wrapper that runs the tools in the test database container.

Epoch doctor check (owner decision 2026-11-18, open since batch 30 item 13): `server/src/epoch_check.rs` combines 05's `EpochAdmin::marker_status` with 04's `BlobInventory` (a first inventory page of one committed row means the volume holds blobs) and 05's `epoch_findings`; tag `restore`; placed after the missing-blob report in `DoctorChecks`. AZ-3666 AC-1's doctor test now passes.

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | High | Spec-Gap | doctor check list | No epoch check was registered, so a restored database below the marker gave no doctor Fail (AZ-3666 AC-1, AZ-3779 AC-6) | **Fixed** (owner decision): server-side `EpochCheck` |
| 2 | Medium | Test infrastructure | `server/tests/root_support/mod.rs` | In-process composition built a `SystemClock` even in a `test-clock` build, so `api_services` failed there ("missing OffsetClock") | **Fixed** (parent): cfg'd `register_clock` |
| 3 | Low | CI | `ci.yml`, `rust-lint.sh` | Feature `test-clock` was neither linted nor tested in CI | **Fixed**: clippy step 4/6 and a `rust-test` step |
| 4 | Low | Docs | `common-helpers/01_helper_config.md` | New key `test_clock_addr` missing from the key table | **Fixed** (parent) |
| 5 | High | Test (regression from batch 89) | `server/tests/migrate_interrupted.rs` killed-migration test | It locked `vaults` and checked `nodes.vault_id` because the newest migration used to alter them; since AZ-4332's migration (on `jobs`) is the newest, `migrate` never waited and the test timed out. Batch 89 ran the touched server binaries but not the whole server crate | **Fixed** (parent): an ACCESS SHARE lock on every application table (sqlx's table excepted) and a schema fingerprint (columns and constraints) compared before and after the kill, so the test holds for any newest migration; 3/3 |
| 6 | Low | Process | batch run scripts | A run script ended each step in `tail`, so its log grew only per step (the long-jobs rule's buffering trap), and a parallel default-feature build overwrote the `sunnycloud` binary under a running `--features vault` step (one false failure of `local_commands`) | Re-run sequentially; future run scripts stream line-buffered and run feature builds one at a time |

## Test Suite

- Whole `sunnycloud-server` crate, default build, run alone: 282 passed, 0 failed (after the `migrate_interrupted` fix).
- `--features vault` (`vault_http`, `vault_report`, `local_commands`, `api_services`, `composition_root`): 26 passed.
- `--features test-clock` (`test_clock_serve`, `api_services`, `composition_root`, `serve_start`, `serve_stop`): 28 passed; `test_clock_serve` 3 runs.
- `pipeline_db_restore`: 8 passed (with the PG 18 tools through `SC_PG_TOOLS`).
- `sc_config` default and `--features test-clock`; `sunnycloud-http-api --features test-clock`; `sunnycloud-sync-feed --lib`: all pass.
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault` and `--features test-clock`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning`, shellcheck on `install-tools.sh` and `rust-lint.sh` exit 0. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The log subscriber of `sc_observability::init_logging` builds its own `SystemClock` for line timestamps; it decides nothing | AZ-4334 AC-2 | Left as is | clear |
| 2 | AZ-3666 scope step 3 and AC-4 name `BlobInventory::find_missing` through `TreeReader::describe_blobs`; the real restore check is `MissingBlobsReport` (`scan_page` + `describe_blob_revisions`) | AZ-3666 | Tested against what exists; spec corrected | clear |
| 3 | AC-73 "files marked unavailable": the listing has no availability field; NFT-RES-37 asks only for 200 and the files listed | AZ-3666 AC-73 | Asserted listed, 200, and `unavailable_at` on the blob row; no field invented | unclear |
| 4 | 05's bump is `GREATEST(max(epoch)+1, unix_seconds)` and never reads the marker, so "the marker only ever rises" holds only while the clock does not go back | AZ-3666 NFR | Noted | unclear |
| 5 | "The volume holds blobs" is read from 04's committed-blob rows (first inventory page), not from the files on the volume | Epoch check | Kept: 04 owns the volume layout; the missing-blob report covers rows without files | clear |
| 6 | Side effects of the CI installer trial on this host: an empty `~/.gnupg` keybox and a pulled `ubuntu:24.04` image | none | Informational | clear |

## Commit

`[AZ-4334] [AZ-3666] serve test clock, DB restore pipeline and doctor epoch check` — Loop: 1

## Next Batch: AZ-3663 (GC, retention and restore pipeline; now unblocked by AZ-4334)
