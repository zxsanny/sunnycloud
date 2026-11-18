# Batch Report

**Batch**: 97
**Tasks**: AZ-3997_camera_folder_layout, AZ-4002_native_library_ci (two parallel workers on disjoint paths; parent applied one cross-boundary patch, reviewed and gated)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3997 | Done; batch retry and a stored large-upload session after a recreated folder proven against fakes only | `mobile_backup/src/backup/{pipeline,sink,folder,verdict}.rs`, `checkpoint/{asset_repo,mod,upload_name}.rs`, `checkpoint/schema_v2.sql` (index, schema version 2), `tests/run_backup/{camera_folder,upload_names}.rs`, fakes, `sc_names` dependency, docs | 21 unit, 62 run_backup (64 with uniffi) | Discovered #1-#3 |
| AZ-4002 | Done except: release signing NOT DONE (owner keys, R18); the CI runner install path and the job on GitHub NOT RUN; arm ABIs not run on a device | `src/mobile/**` (Flutter project, Kotlin layer, committed Kotlin bindings), `scripts/ci/mobile-*.sh` (9) and `tests/test-mobile-checks.sh`, `.github/workflows/ci.yml` (job `mobile`), `scripts/ci/README.md`, `mobile_backup` `uniffi-cli` feature and bindgen bin, `uniffi.toml` | below | Discovered #4-#7 |

**Cross-boundary finding of the AZ-4002 worker (applied by the parent):** the Kotlin generated from the AZ-3991 surface did not compile with uniffi 0.32.2, which the spike (AZ-3991, bindings only generated, never compiled) had not noticed. (1) `ForeignAssetSource` failed with sync-core's `HostError`, an error type of another crate: uniffi emits a converter of the wrong crate's `RustBuffer`. Fix: a local `AssetSourceError` (NotFound, PermissionDenied, Failed) in `mobile_backup`; Kotlin hosts throw `AssetSourceException`. (2) `BackupError::UpgradeRequired { message }` clashes with `Throwable.message`: `uniffi.toml` renames the Kotlin field to `userMessage`. The patch touched `ffi.rs`, `lib.rs`, one test and added `uniffi.toml`; it was applied after AZ-3997 finished, and the committed bindings still match the combined surface.

## AZ-3997

| AC | Test | Result |
|---|---|---|
| AC-1 | `upload_names`: same display name gets `<stem> (<asset_id>)<ext>` and the first keeps its name; case-only difference counts; names kept across runs and edits; the check uses the index (EXPLAIN); unit tests of suffix placement and the 255-byte limit | pass |
| AC-2 | `camera_folder`: `ParentGone` and `ParentNotFound` on the block path and `parent_deleted` / `parent_not_found` batch entries: exactly one folder create, new id stored in `cursor`, 2 attempts for the refused asset, later assets go to the new folder | pass |
| AC-3 | a second refusal ends the run with the error noted: exactly 2 attempts, asset pending, `last_error` with a correlation id | pass |

## AZ-4002

| AC | Evidence | Result |
|---|---|---|
| AC-1 | `mobile-build-app.sh` debug (parent) and release (worker): `mobile-check-apk.sh` finds minSdk 30, no `android:process`, one library per ABI; the Kotlin smoke test loads the library and `BackupCore.status` answers empty without a server; the release APK ran on the 16 KB emulator and `nativeInit` answered true; `connectedDebugAndroidTest` 2 of 2 there | pass; arm ABIs and a 4 KB image not run |
| AC-2 | 3 built libraries and 10 libraries in the debug APK are `0x4000` aligned (`libflutter.so` `0x10000`); `zipalign -P 16` passes; seeded 4 KB library, a directory holding it and an empty directory fail | pass |
| AC-3 | an unchanged surface passes; a renamed `BackupCore.status` in the Rust source and a deleted generated file fail | pass |
| AC-4 | `mobile-check-no-vault.sh` over the 3 ABIs: 170 crates, `vault-core` absent; a seeded dependency fails naming `vault-core` | pass |
| AC-5 | job `mobile` runs AC-1..AC-4, Kotlin tests, `flutter analyze`, `flutter test` and the seeded-failure script under `watchdog.sh`; `check-pinning` 0 unpinned | passes locally; never run on a GitHub runner |

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | High | Correctness | AZ-3991 `ffi.rs` | non-compiling Kotlin from the committed surface | **Fixed** (patch applied, contract updated) |
| 2 | Medium | Evidence | AZ-4002 | release signing, runner install path and the job on GitHub not run | Kept (owner keys, first CI run) |
| 3 | Medium | Evidence | AZ-3997 | batch retry and a session surviving a changed parent only against fakes | Kept (E-31 leftover) |
| 4 | Low | Behaviour | AZ-3997 | names equal only after non-ASCII folding or NFC are not caught and fall to the server's conflict-copy rule | Accepted, documented |
| 5 | Low | Supply chain | Gradle and pub | no hash verification metadata for dependencies | Optional hardening task |

## Test Suite

Parent, on the combined tree: `cargo fmt --all --check`; `mobile_backup` tests default (21 unit, 3 graph, 62 run_backup, 6 set_server) and `--features uniffi` (64 run_backup); workspace clippy `-D warnings`; clippy `-p mobile_backup` with `uniffi`, `uniffi-cli`, `allow-plain-http`; server `vault` and `test-clock`; `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `check-pinning`, `build-wasm`, `cargo deny check licenses bans`; `mobile-build-native.sh`, `mobile-check-bindings.sh`, `mobile-check-no-vault.sh`, `mobile-test.sh` (flutter analyze clean, flutter test 1, Kotlin 2), `mobile-build-app.sh debug` and `mobile-check-apk.sh`, `test-mobile-checks.sh`, `test-check-pinning.sh`, `test-ci-no-quiet.sh`: exit 0. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Name collision check folds ASCII case only (`lower()`); an exact check needs a `name_key` column that a migration cannot backfill | AZ-3997 | accepted; revisit with a column if hostile names matter | unclear |
| 2 | An edited asset keeps its own chosen name | AZ-3997 flagged concern | as built | clear |
| 3 | Whether the real server accepts a second stage and commit for the same key after a refused entry, and whether a session survives a changed parent | AZ-3997 | verify on the E-31 stack | unclear |
| 4 | Gradle needs a JDK with `jmods`: JDK 21 Temurin, not the local JDK 26 | AZ-4002 | `mobile-common.sh` and the job use Temurin 21 | clear |
| 5 | A future foreign trait of `mobile_backup` failing with a sync-core error type hits the same uniffi limitation | c08 credential provider | define errors in `mobile_backup` | clear |
| 6 | `release` is debug-signed when `key.properties` is absent | R18 | owner supplies keys; consider failing a "shippable" CI flag | unclear |
| 7 | Gradle and pub dependencies have no hash verification | pinning rules | optional hardening task | unclear |

## Commit

`[AZ-3997] [AZ-4002] Camera folder names and recovery; mobile project and native library CI` — Loop: 1

## Next Batch: recompute (AZ-4000 flutter channel bridge, AZ-3992 and later E-27 tasks as dependencies clear)
