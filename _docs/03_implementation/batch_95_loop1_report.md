# Batch Report

**Batch**: 95
**Tasks**: AZ-3990_run_backup_loop (one worker on `src/mobile_backup/**`; parent review and gate)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3990 | Done; real-server legs of AC-78 and AC-5 NOT-RUN here | `src/mobile_backup/src/{api,source,outcome}.rs`, `backup/{mod,discovery,folder,pipeline,sink}.rs`, `checkpoint/asset_repo.rs`, `tests/run_backup/**`, `Cargo.toml` (dev-dependencies), `Cargo.lock`, `backup_core.md` | 25 new + 19 existing | Discovered #1-#6 |

No schema change. The loop is a plain Rust API: run mutex (`AtomicBool` with a drop guard, one `Backup` per process), discovery from the change token with rows stored before the token moves, visible-id diff, hash-ahead window from `AndroidConfig.hash_ahead`, hash and key stored before upload, small-batch and large paths, `uploaded` only after the acknowledgement, a lazily created `Camera Uploads/<device label>/` folder whose node id is kept in `cursor`, and `resume_batches` at the start of every run. A narrow trait `BackupApi` (9 methods, implemented for `HttpSyncClient`) is the seam the tests fake, because `upload`, `upload_small` and `resume_batches` are inherent methods of `HttpSyncClient` and not of `SyncApi` (open point 9 of `sync_api.md`); sync-core is unchanged.

| AC | Test (`tests/run_backup/`) | Result |
|---|---|---|
| AC-1 | `one_run`: second run while one runs is skipped with no sync call; a run after a restart waits for no lease | pass |
| AC-2 | `new_assets`: rows exist before the token advances (a trigger makes the token write fail); token bytes round-trip; nothing new writes nothing | pass |
| AC-3 | `visible_diff`: queued on a count change, on catch-up, none when unchanged, access-level change triggers it | pass |
| AC-4 | `hash_ahead`: window, hashes stored before upload, nothing hashed twice after restart; large file keeps its block hashes; passed deadline ends partial | pass |
| AC-5 | `ack_before_uploaded`: block path `NothingToSend` after a failed row write (same key, 0 bytes, one server file); batch replayed with its `commit_id`, no second file | pass (fake server) |
| AC-6 | `edits_and_pending`: double edit uploads once with the latest hash; re-edit re-uploads; pending and trashed rows wait; `AssetGone` skipped | pass |
| AC-7 | `routing`: size routing, batch size by the server limit, `client_meta` present | pass |
| AC-8 | `folder`: created once, id kept, reused; existing folder found; no upload creates no folder | pass |
| AC-78 | `two_processes`: two `Backup` instances, separate checkpoints, one idempotent fake server: 100 assets, 300 assets of 1 KiB to 1 MiB, block-path assets: exactly one server file each | pass (fake server) |

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Evidence | AC-78, AC-5 | the real-server legs (IT-17-3, FT-P-76, kill after ack) are not run; the fakes do not prove them | Kept; belongs to the E-31 stack (leftover below) |
| 2 | Low | Error handling | `backup/mod.rs` resolver | `open_read(id).ok()` drops the error, but `AssetGone` is its only error and `None` is the resolver's contract for a gone file; the left-out files are logged | Accepted |
| 3 | Low | Behaviour | permanent per-asset errors | `SizeTooLarge`, `InvalidName`, `SourceChanged` and `Io` end the run with `Err` and the asset is retried every run until c07 maps them to `skipped_*` and pauses | Accepted, c07 scope (Discovered #2) |

## Test Suite

`cargo test --locked -p mobile_backup -- --test-threads=1`: 16 unit, 3 crate-graph, 25 `run_backup`, green (worker ran it 3 times, parent once; parallel once). CI-parity (parent): `cargo fmt --all --check`, workspace clippy `-D warnings`, `-p mobile_backup --features uniffi`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `check-pinning`, `build-wasm`, `cargo deny check licenses bans`, Android aarch64 build with `uniffi` exit 0. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `AssetSource` needed `visible_count` and `describe`; `RunContext` lacks `network_policy` | AZ-3990 AC-3 | Added to the contract text; `network_policy` with c07 | clear |
| 2 | Permanent per-asset sync errors stop the run and the asset is retried every run | excluded to c07 | c07 maps to `skipped_*` and pauses | clear |
| 3 | Source of `device_id` | flagged concern, Low | constructor input; owner decides `cursor` vs sign-in result | unclear |
| 4 | When an asset leaves the visible set, and whether a relisted `skipped_*` asset retries | flagged concern, Medium | not handled | unclear |
| 5 | Hosts must return strictly increasing tokens and not split equal tokens across a page | `assets_after` | documented in `backup_core.md` | clear |
| 6 | `NothingToSend` gives no node id; the row keeps the revision only | `sync_api.md` | add `get_revision` if c14 needs the node | unclear |

## Commit

`[AZ-3990] run_backup loop: change token, visible-id diff, hash-ahead, idempotent uploads` — Loop: 1

## Next Batch: AZ-3991 backup_core_surface; then AZ-4002
