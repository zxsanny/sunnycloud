# Vault core in the mobile native library [v1.2]

**Task**: AZ-4025_mobile_vault_core_library
**Name**: Vault core in the mobile library
**Description**: In v1.2 builds, compile `vault-core` with its UniFFI feature into the same per-ABI library as `mobile_backup` and sync-core, commit its Kotlin bindings under the drift check, run the vault vector file through the packaged library on the emulator, and keep v1.0 and v1.1 builds free of vault code
**Complexity**: 3 points
**Dependencies**: AZ-4002_native_library_ci, AZ-4014_vault_uniffi_adapter, AZ-4015_vault_vectors_ci_gate, AZ-4003_android_emulator_harness, AZ-3968_sync_uniffi_surface
**Component**: 17 android_app (native library build) [v1.2]
**Tracker**: AZ-4025
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/n01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — "UniFFI form" (E-29/c11).
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — one `uniffi` version, drift-check rule.
- `_docs/02_document/components/19_vault_core/description.md` §5 ("Mobile: UniFFI bindings ... compiled into the same library as 13 (assembled by 17)").

## Problem

19 §5 says the vault core's UniFFI bindings are compiled into the same library as sync-core, "assembled by 17", and the layering table lets `mobile_backup` import `vault-core` in v1.2. AZ-4014 produces the bindings and tests them on a JVM, and AZ-4002 builds the Android library and checks that v1.0 links no vault code, but no task puts `vault-core` into the library for v1.2 (both flag it), so the UniFFI surface has no production caller on the phone. A second library for the vault would load a second copy of the runtime and break "one library per ABI, loaded once per process" (AZ-4002).

## Outcome

- The v1.2 Android build produces one library per recorded ABI that contains `mobile_backup`, sync-core and `vault-core` with its `uniffi` feature, loaded once per process.
- Kotlin bindings of `vault-core` are generated from the Rust sources with the workspace UniFFI version (0.32.2), committed, and covered by the drift check of AZ-4002.
- The 19 vector file runs through the packaged library on the emulator with results identical to native.
- v1.0 and v1.1 builds still link no `vault-core` (AZ-4002 AC-4 stays green); the build scripts that E-28/c03 reuses carry the same release switch.

## Scope

### Included
- The v1.2 variant of the library build and packaging; binding generation for `vault-core`; extension of the drift, alignment and no-vault checks to both variants; the emulator vector run in the `mobile` CI job.

### Excluded
- The vault operations the app needs on top of the core (E-27/n02) and the vault screen (E-27/n03).
- The iOS static library (E-28/c03, AZ-3975; see Flagged concerns); the UniFFI exports themselves (AZ-4014); the vector file (AZ-4015).

## Acceptance Criteria

**AC-1: One library with the vault core (19 §5)**
Given a clean checkout and the v1.2 build variant
When the debug and release builds run
Then each recorded ABI has exactly one native library, its linked crates include `mobile_backup`, sync-core and `vault-core`, and a smoke test on the emulator calls a sync-core function and the vault core's passphrase check from Kotlin in the same process without a server

**AC-2: No vault code before v1.2 (ADR-016, AZ-4002 AC-4)**
Given the v1.0 and v1.1 build variants
When their linked crates are listed
Then `vault-core` is absent from both, and a seeded dependency on it fails the check

**AC-3: Bindings under the drift check**
Given the committed `vault-core` Kotlin bindings
When the drift check regenerates them
Then an unchanged surface passes, a seeded change to a `vault-core` export fails, and the lockfile holds one `uniffi` version

**AC-4: Alignment of the larger library (R31, IT-17-16 part)**
Given the v1.2 libraries
When the 16 KB alignment check runs
Then every loadable segment of every library is 16 KB aligned

**AC-5: Vectors through the packaged library (19 §5 "same test vectors on all three", IT-19-1)**
Given the 19 vector file and the v1.2 app on the API 34+ emulator
When every vector runs through the Kotlin bindings of the packaged library
Then every output equals the native output case by case, and a seeded wrong vector fails the run

**AC-6: CI job**
Given a pull request to `dev`
When the `mobile` job runs
Then it builds both variants and runs AC-2 to AC-5 with verbose output, and any failure fails the job

## Non-Functional Requirements

**Compatibility**
- UniFFI 0.32.2, one version in the lockfile; NDK r28 or newer; JNA 5.17.0 or newer.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Linked-crate check on both variants and a seeded build | Pass, pass, fail |
| AC-3 | Drift check on real and seeded `vault-core` surfaces | Pass, then fail |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | v1.2 build on the emulator | Smoke call into sync-core and vault-core | One library; both answer | ADR-013 |
| AC-4 | v1.2 libraries | Alignment check (IT-17-16 part) | 16 KB aligned | R31 |
| AC-5 | Vector file, emulator | IT-19-1 through the packaged library | Equal to native | AC-60, AC-61 |
| AC-6 | Pull request | `mobile` job | Both variants checked | IT-17-16 |

## Runtime Completeness

- **Named capability**: the vault core in the mobile native library (19 §5).
- **Production code that must exist**: the v1.2 library build and packaging used by release builds.
- **Allowed external stubs**: none.
- **Unacceptable substitutes**: a second native library for the vault, a Kotlin or Dart reimplementation of any vault primitive, a library built outside CI for release.

## Constraints

- UniFFI only, no flutter_rust_bridge (ADR-013); Dart never loads the library.
- `src/mobile/ios/` belongs to 18; this task changes no Swift code.

## Risks & Mitigation

**Risk 1: Library size and memory**
- *Risk*: adding `vault-core` (Argon2id, XChaCha20-Poly1305) grows the library loaded by the backup worker.
- *Mitigation*: AC-1 records the size per ABI; the backup memory checks of E-27 run on the v1.2 variant too.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How vault code is kept out of v1.0 and v1.1 and added in v1.2 (a Cargo feature, a build variant or separate release branches) is module-layout open point 6 and ADR-016; this task checks only the result | Owner / E-22, E-27, E-29, E-30 | open | Medium |
| The iOS static library (E-28/c03, AZ-3975) needs the same v1.2 addition; it is E-28's task and is not changed here | E-28 owner | open | Medium |
| The mobile randomness source for the vault (foreign callback or OS source) is open (`vault_core_api.md` open point 5) | Owner / reviewer | open | Low |
| AZ-4002 and AZ-4014 carry open rows for this gap; they can be resolved once this task has a key | Parent (Step 4) | open | Low |
