# Batch Report

**Batch**: 98
**Tasks**: AZ-4000_flutter_channel_bridge (one worker on `src/mobile/**`; parent review and gate)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4000 | Done for the bridge, `setServer`, `updateSettings`, the status stream, native settings and the no-token checks; six commands answer `notImplemented` (owned by later tasks); emulator blackbox NOT-RUN (leftover) | `src/mobile/android/app/src/main/kotlin/org/sunnycloud/backup/{ChannelContract,SettingsStore,Host,StatusChannel,BackupApplication,MainActivity}.kt`, `lib/src/{backup_status,backup_channel}.dart`, Kotlin and Dart tests, `status_channel.md`, 17 description | Kotlin 32 (30 new), Dart 20 (18 new) | Discovered #1-#4 |

No Rust surface change and no generated-binding change (`mobile-check-bindings.sh` still passes). Each event is a full snapshot read from `BackupCore.status()`; whoever changes status calls `StatusChannel.publish()`. Errors map from `BackupException` with an exhaustive `when` (a new variant breaks the build); an unexpected failure reaches Dart as `Unexpected` with no text and is logged natively.

| AC | Test | Result |
|---|---|---|
| AC-1 | snapshot on subscribe and on each `publish`, none after cancel, the counts add up (JVM with a fake core; Dart parse) | pass at unit level; the emulator run with a real upload NOT RUN |
| AC-2 | `setServer` (real core refuses `http://` with `HttpsRequired`, typed failures, server text kept for `UpgradeRequired`, invalid `url`) and `updateSettings` (stored, returned, published; 5 invalid shapes leave the store unchanged); a command with no native owner answers `notImplemented` | pass for the two commands; the other six NOT DONE here |
| AC-3 | reflection over wire fields and commands finds nothing named like a token, code or verifier; a seeded token field fails the check; a sentinel scan of every sent payload; a Dart source scan; a mutation (error carrying the exception text) failed 2 tests | pass; no real sign-in exists yet to put a real token through |
| AC-4 | settings written by one store are read by a new store on the same file; separators and Unicode in bucket names; empty selection differs from no selection | pass at unit level |

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Scope | spec "command set" | six commands of `status_channel.md` have no native implementation in this task | Accepted: each belongs to a later task that owns the native side (leftover names them) |
| 2 | Medium | Design | `BackupCore` | no status-change callback, so the bridge relies on `publish()` calls | Accepted; a Rust listener is a surface change for the owner |
| 3 | Low | Error handling | `StatusChannel.kt`, `backup_channel.dart` | the catch blocks convert to typed errors or error events and log; none swallows | Accepted |
| 4 | Low | Performance | `snapshot()` | reads SQLite counts on the main thread | Not measured; move to an executor if a large checkpoint makes it slow |

## Test Suite

Parent: `mobile-test.sh` (flutter analyze clean, `flutter test` 20, Kotlin 32), `mobile-check-bindings.sh`, `mobile-build-app.sh debug` and `mobile-check-apk.sh` exit 0; the worker ran the suites 3 times. No Rust or workspace file changed, so the cargo gates were not re-run.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | "Another asset uploaded, another snapshot" needs a change source: the core has none and `run_backup` is not exported | AC-1 | c10/c11 call `publish()`, or the core gets a status listener | unclear |
| 2 | `signed_in`, `device_label`, `running` have no source until c08 and c10 | status_channel.md | `NativeState` seam, fixed answers | clear |
| 3 | Default `buckets`: the mockup shows Camera and Screenshots, the default is not stated | AZ-3993 flagged | `null` means all albums until the owner decides | unclear |
| 4 | `battery_exempt` can change in system settings with no app event | status_channel.md | c17 calls `publish()` on return | clear |

## Commit

`[AZ-4000] Flutter channel bridge: status events, settings and the no-token checks` — Loop: 1

## Next Batch: nothing ready that the agent can run; see the blockers in the state file
