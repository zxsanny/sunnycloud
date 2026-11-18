# Mobile project scaffold and native library CI: per-ABI build, 16 KB alignment, UniFFI drift check

**Task**: AZ-4002_native_library_ci
**Name**: Mobile scaffold and native library CI
**Description**: Create the shared Flutter project with its Kotlin Android layer (minSdk 30, one process), build `mobile_backup` with sync-core into one library per ABI that the app loads through JNA, and gate it in the `mobile` CI job with a 16 KB alignment check, a Kotlin binding drift check and a check that v1.0 links no vault code
**Complexity**: 5 points
**Dependencies**: AZ-3985_android_uniffi_device_spike, AZ-3989_mobile_backup_checkpoint_db, AZ-3968_sync_uniffi_surface, AZ-3761_ci_skeleton
**Component**: 17 android_app (Flutter project, Kotlin layer, native library build)
**Tracker**: AZ-4002
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c18

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — the `uniffi` feature and its version (E-23/c15).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — the surface whose bindings are checked.
- Record of E-27/c01 (NDK, ABIs, link setting, JNA, process model); `deployment/ci_cd_pipeline.md` (`mobile` job is E-27's per AZ-3652).

## Problem

No task creates the mobile project: E-00 leaves `src/mobile/` and the `mobile` CI job to E-27 (AZ-3652). Every Kotlin and Flutter task needs a project that builds, loads the one Rust library per ABI and runs its tests in CI. A library that is not 16 KB aligned fails on Android 15 devices with 16 KB pages (R31), and Kotlin bindings that fall behind the Rust surface break at run time. v1.0 and v1.1 builds must not link `vault-core` (module-layout exception 6, ADR-016). The row was planned at 3 points for the library CI; it also carries the project scaffold, which no row owns, so it is re-estimated at 5.

## Outcome

- A Flutter project with the Kotlin Android layer: minSdk 30, the app in one process (per the E-27/c01 record), JNA 5.17.0 or newer, release signing configuration placeholders without secrets.
- One library per recorded ABI containing `mobile_backup` and sync-core, packaged into the app and loaded once per process.
- Kotlin bindings generated from the Rust sources and committed; a drift check fails when they differ.
- The `mobile` CI job: Rust build per ABI, alignment check, binding drift check, Kotlin unit tests, `flutter test`, a debug and a release build; verbose output.
- A check that the v1.0 Android build links no `vault-core`.
- The shared Dart app layout where 18 later adds its iOS screens (shell in E-27/c15).

## Scope

### Included
- Project creation, Gradle setup, manifest basics (one process, minSdk 30), library packaging per ABI.
- Build scripts reused by E-28/c03 for the iOS static library.
- The alignment, drift and no-vault checks with seeded failures.
- The `mobile` CI job in the PR gate.

### Excluded
- App features (other E-27 tasks); the emulator harness (E-27/c19).
- Play signing keys and store upload (owner, R18).
- The iOS library build (E-28/c03).

## Acceptance Criteria

**AC-1: Project and library build**
Given a clean checkout with the pinned NDK and Flutter versions
When the debug and release builds run
Then both produce an app with one library per recorded ABI, the app declares minSdk 30 and one process, and a smoke test loads the library and calls `BackupCore::status` without a server

**AC-2: 16 KB alignment gate (IT-17-16 part, R31)**
Given the built libraries
When the alignment check runs
Then every loadable segment of every library is 16 KB aligned, and a library seeded with 4 KB alignment fails the check

**AC-3: Binding drift gate**
Given the committed Kotlin bindings
When the drift check regenerates them from the current Rust surface
Then an unchanged surface passes, and a seeded change to a `BackupCore` method fails the check

**AC-4: No vault code in v1.0**
Given the v1.0 Android build
When its linked crates are listed
Then `vault-core` is absent, and a seeded dependency on it fails the check

**AC-5: CI job**
Given a pull request to `dev`
When the `mobile` job runs
Then it runs AC-1 to AC-4, the Kotlin unit tests and `flutter test` with verbose output, and any failure fails the job

## Non-Functional Requirements

**Compatibility**
- UniFFI 0.32.2 and one `uniffi` version in the workspace lockfile; JNA 5.17.0 or newer; NDK r28 or newer.

**Reliability**
- No quiet flags in build or test commands.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Alignment check on real and seeded libraries | Pass, then fail |
| AC-3 | Drift check on real and seeded surfaces | Pass, then fail |
| AC-4 | Linked-crate check on real and seeded builds | Pass, then fail |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Clean checkout | Debug and release builds; smoke call | Builds; `status` returns | ADR-013 |
| AC-5 | Pull request | `mobile` job | All checks run; failures fail the job | IT-17-16 |

## Constraints

- UniFFI only; no flutter_rust_bridge (ADR-013, S4-4); Dart never loads the library.
- `src/mobile/ios/` is owned by 18; this task creates no Swift code.

## Runtime Completeness

- **Capability**: the per-ABI native library with UniFFI bindings loaded through JNA.
- **Production code that must exist**: the library build and packaging used by release builds.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: a Kotlin or Dart reimplementation of any core call; a library built outside CI for release.

## Risks & Mitigation

**Risk 1: NDK or JNA update breaks alignment**
- *Risk*: a toolchain bump silently drops the 16 KB setting.
- *Mitigation*: AC-2 gates every pull request.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How vault code is kept out of v1.0 and v1.1 builds and added in v1.2 is module-layout open point 6 (E-22, E-27, E-29, E-30); AC-4 checks only the result | Owner / E-27, E-29 | open | Low |
| No E-27 or E-29 task adds `vault-core` to the mobile library for v1.2 or builds a mobile vault UI (see E-29/c11) | Owner | open | Medium |
