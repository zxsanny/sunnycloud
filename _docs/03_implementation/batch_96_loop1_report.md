# Batch Report

**Batch**: 96
**Tasks**: AZ-3991_backup_core_surface (one worker, resumed once; parent review and gate)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3991 | Done; `run_backup` itself is not exported (needs the c08 credential provider), real-server legs NOT-RUN | `src/mobile_backup/src/{backup_core,error,server,status,ffi}.rs`, `backup/{verdict,unreadable}.rs`, `outcome.rs`, `source.rs`, `api.rs`, `backup/*`, `checkpoint/asset_repo.rs`, `Cargo.toml`, `Cargo.lock` (adds `async-compat`), `tests/set_server.rs`, `tests/run_backup/**`, `backup_core.md` | 18 unit, 3 graph, 52 run_backup, 6 set_server (54 run_backup with `uniffi`) | Discovered #1-#8 |

No schema change. `BackupCore` (`new`, async `set_server`, `status`, `cancel`), the final wire-error table applied inside the loop (per-asset errors now end the asset `skipped_*` or `failed` and the loop goes on; pauses stop the run), `RunContext` with a network policy and a time budget, the `ForeignAssetSource` twin with an adapter so a Kotlin exception is a typed error and not a panic. The first hand-back deferred the foreign `AssetSource` export, which the spec lists as included; the worker was sent back and finished it.

| AC | Test | Result |
|---|---|---|
| AC-1 | `set_server`: plain http refused with 0 connections on a listening socket (release); with `allow-plain-http` it gets past the scheme check | pass |
| AC-2 | `set_server` (unreachable and invalid address on the real client; server-info and upgrade-required rows at the mapping boundary through a fake probe) | pass, real-server leg NOT-RUN |
| AC-3 | 17 tests in `run_backup/wire_errors.rs` plus `foreign_source` (host errors) | pass |
| AC-4 | `counts`: the counts add up at over 100 status reads of two runs with vanished assets and a full quota | pass |
| AC-5 | `cancel`: three staged uploads in flight end the run partial and send nothing more; during a block upload; with no run | pass (fake client) |
| AC-6 | `no_token`: sentinel tokens appear in no outcome, status, error or captured log | pass (regression guard; end to end is ST-17-1 in AZ-3992) |

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Spec gap | hand-back 1 | foreign `AssetSource` export deferred though listed as included | **Fixed** (resumed worker) |
| 2 | Medium | Evidence | AC-2, AC-5 | real https server and real `HttpSyncClient` cancel not exercised | Kept, E-31 and AZ-3992 |
| 3 | Low | Behaviour | `HttpSyncClient::cancel` | cancel is permanent, so a cancelled `Backup` needs a new client; the old doc sentence was wrong and is replaced | Accepted, documented |
| 4 | Low | Evidence | Kotlin bindings | generated once from the built library (3 files, no errors); the drift check is c18 | Accepted |

## Test Suite

CI-parity (parent, after the last change): `cargo fmt --all --check`, tests default and `--features uniffi`, workspace clippy `-D warnings`, clippy with `uniffi`, `allow-plain-http`, server `vault` and `test-clock`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `check-pinning`, `build-wasm`, `cargo deny check licenses bans`, Android aarch64 build with `uniffi`: exit 0. `CI-parity: PASS`. The new tests ran 3 times each.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `run_backup` is not exported: it needs a connected client and credential provider (c08) | spec Included | export with a connected-client factory in c08 | unclear |
| 2 | `FreeSpaceFloor` has no `PauseReason` | open point 7 | mapped to `Paused(QuotaExceeded)` with its own text | unclear |
| 3 | `BackupError` gained `InvalidServerUrl` and `CheckpointUnavailable`; connect errors without a row map to `ServerUnreachable` | contract | non-breaking additions | unclear |
| 4 | `set_server` stores nothing; where the address lives is open | open point 1 | c08 and c11 decide | unclear |
| 5 | `ServerCheck.trust` is always `System` until c12 | spec | as specified | clear |
| 6 | `Transport` and `RateLimited` end the run with `Err`, not a pause | contract | matches `ServerUnreachable` | clear |
| 7 | `LimitedAccess`, `ServerRejectedName`, `SignedOut` have no trigger | open point 6 | unused | unclear |
| 8 | A host `Failed` on `open_read` ends the asset `failed` (retried only after a token change) so one bad asset cannot block the others | risk 1 | accepted | clear |

## Commit

`[AZ-3991] BackupCore surface, wire-error mapping and the foreign AssetSource` — Loop: 1

## Next Batch: AZ-4002 (mobile scaffold and native library CI), AZ-3992
