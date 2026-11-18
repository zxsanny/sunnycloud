# Batch Report

**Batch**: 94
**Tasks**: AZ-3989_mobile_backup_checkpoint_db (parent, inline; built as specified, complexity budget)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3989 | Done | `src/mobile_backup/` (manifest, `lib.rs`, `config.rs`, `checkpoint/{mod.rs,repo.rs,schema.sql}`, unit tests, `tests/crate_graph.rs`), `shared/config/src/table.rs` (4 rows), `src/.cargo/config.toml` (16 KB flag for the Android targets), `scripts/ci/rust-lint.sh` (clippy with `--features uniffi`), docs: `01_helper_config.md`, 17 description section 4 and 5, `backup_core.md`, 17 record correction | 16 + 3, kill test 6 runs | Discovered #1-#3 |

Built as specified: one crate, a `CheckpointDb` that opens on first use (WAL, `synchronous=FULL`, one connection so the crate is the only writer, forward migrations by `PRAGMA user_version`, a newer file refused before any write), `SqliteCheckpointStore` over `session` and `batch`, and `AndroidConfig` over four new rows of the shared key table. No new layer beyond what the spec names.

| AC | Test | Result |
|---|---|---|
| AC-1 | `cdylib` built for aarch64, armv7 and x86_64 Android (`--locked --release --features uniffi`, NDK r28.2, every `LOAD` segment `0x4000`) and `staticlib` on the host; `crate_graph.rs`: the tree (default and `uniffi`) holds sync-core, sync-core-proto and `sc_*` helpers only, no `contracts`, `vault-core` or `sunnycloud*`; a negative test shows the check names a forbidden crate | pass |
| AC-2 | new database has `asset, batch, cursor, session` at `SCHEMA_VERSION`; reopen applies nothing; an older version (injected two-migration list) migrates with its row intact; a newer version is refused with `NewerSchema` and the db and wal bytes are unchanged | pass |
| AC-3 | constructing the db and the store leaves the directory empty | pass |
| AC-4 | the same contract cases (save, load, overwrite, clear, absent keys, three batches, `load_batches` order, `clear_batch`) answer identically on `SqliteCheckpointStore` and `MemoryCheckpointStore`; every record survives a store restart; a malformed row is an error, never a wrong checkpoint | pass |
| AC-5 | 10 `kill -9` of a separate writer process at random moments (after its first acknowledged step; one transaction inserts an asset, a batch row and the step counter): every reopen passes `integrity_check`, holds every acknowledged step, and `asset = batch = counter` (no partial transaction); 6 runs | pass |
| AC-6 | 50,000 filled asset rows, `wal_checkpoint(TRUNCATE)`: file under 10 MiB | pass |
| AC-7 | defaults 15m, true, false, 8; an unknown `SUNNYCLOUD_ANDROID_BOGUS` refuses the load naming it; `hash_ahead` 0 is refused | pass |

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Spec gap | 17 section 10 vs key table | The docs write `android.catchup_interval`; the table is flat snake case with `SUNNYCLOUD_<KEY>` environment names, so the keys are `android_*` | **Fixed**: rows added as `android_*`, noted in `01_helper_config.md`, flagged concern 3 of the spec (`mobile.*`) left open |
| 2 | Medium | Correctness | AZ-3985 record | The record said NDK r28 links 16 KB alignment by default; true for 64-bit targets only, armeabi-v7a links at `0x1000` | **Fixed**: flag set for all three targets in `src/.cargo/config.toml`, record and batch 93 report corrected |
| 3 | Low | Gate | `check-db-gates` | SQL text outside a repository module or a `tests/` directory | **Fixed**: `checkpoint/repo.rs` and `checkpoint/tests/mod.rs` |
| 4 | Low | CI | `rust-lint.sh` | the `uniffi` feature code is not linted by the default pass | **Fixed**: step 5/7 added |
| 5 | Low | Docs | 17 section 4 | `session` columns did not fit `UploadCheckpoint` | **Fixed** (flagged concern 1): table text updated |

## Test Suite

`cargo test --locked -p mobile_backup -p sc_config -p sync-core -p sync-core-proto` all green (mobile_backup 16 + 3, sc_config 68, sync-core every file). CI-parity: `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `-p mobile_backup --features uniffi`, server `vault` and `test-clock`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `check-pinning`, `build-wasm`, `cargo deny check licenses bans` exit 0. `CI-parity: PASS`. The full workspace suite runs at the end of Step 7.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Flagged concern 2: how Kotlin reads the WorkManager values (`catchup_interval`, `unmetered_only`, `charging_only`) has no getter on `BackupCore` | AZ-3989 | `AndroidConfig` exists; the getter belongs to E-27/c07 | unclear |
| 2 | Flagged concern 3: `android.*` vs `mobile.*` for the shared keys | AZ-3989 | Kept `android_*`; E-27 and E-28 owners decide | unclear |
| 3 | `synchronous=FULL` costs one flush per commit; kill safety needs only WAL (`NORMAL` survives a process kill), `FULL` also survives power loss | 17 section 4 | Kept `FULL`; revisit with E-27/c06 if the per-asset commit rate matters | clear |

## Commit

`[AZ-3989] mobile_backup crate, checkpoint database and CheckpointStore` — Loop: 1

## Next Batch: recompute the ready list
