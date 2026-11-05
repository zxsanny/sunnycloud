# iOS static library and Swift package from the shared Rust crate

**Task**: AZ-3975_ios_uniffi_static_library
**Name**: iOS UniFFI static library
**Description**: Build the shared `mobile_backup` crate (with sync-core) as one static library for iPhone and the simulator, generate its Swift bindings into a Swift package the iOS app links, and gate it on a binding drift check and a licence check
**Complexity**: 3 points
**Dependencies**: AZ-3985_android_uniffi_device_spike, AZ-3989_mobile_backup_checkpoint_db, AZ-3991_backup_core_surface, AZ-4002_native_library_ci, AZ-3968_sync_uniffi_surface
**Component**: 18 ios_app [v1.1] (Swift package for the UniFFI static library)
**Tracker**: AZ-3975
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — the `uniffi` feature, derives and foreign-trait callbacks of sync-core, produced by E-23/c15.
- `_docs/02_document/components/18_ios_app/description.md` §1, §8; ADR-013 (UniFFI for Swift, one static library, no flutter_rust_bridge), ADR-011 (App Store exception).

## Problem

18 adds no protocol code: every upload, hash and token refresh on iOS runs in the shared Rust crate `mobile_backup` (17) on top of sync-core (13). On Android that crate is built as one `cdylib` per ABI by E-27. iOS needs the same sources as one static library with Swift bindings, built for a physical iPhone and for the simulator, before any Swift code can call `BackupCore`. Without a drift check the Swift bindings can fall behind the Rust surface; without a licence check a GPL crate could end up in an App Store binary.

## Outcome

- One static library per iOS target (device, simulator) built from the E-27 sources with the pinned UniFFI version, wrapped in a Swift package the iOS app links.
- Swift bindings generated from the same UniFFI surface Kotlin uses; Swift owns the handle, Flutter never calls Rust.
- CI fails when the committed Swift bindings differ from freshly generated ones, or when the iOS library contains a GPL crate.
- The v1.1 iOS build links no vault code.

## Scope

### Included
- Build of the static library for the device target and the simulator target(s), reusing E-27's native library build scripts and its library layout from the E-27 device spike.
- Swift binding generation and the Swift package that exposes `BackupCore` to the Swift layer.
- A Swift smoke test that creates the core and calls `BackupCore::status` with no network.
- A binding drift check for Swift (same idea as Android's UniFFI drift check in E-27/c18).
- The licence check over the iOS library's dependency tree with the `cargo deny` allowlist (no GPL crates).
- A check that the v1.1 iOS build does not link `vault-core` (module-layout exception 6).

### Excluded
- The Rust crate itself and its UniFFI surface (E-27/c05, E-27/c07) and the sync-core derives (E-23/c15).
- Signing, notarisation, TestFlight and App Store submission (owner items, R18).
- Any Swift platform code (E-28/c04 to c09).

## Acceptance Criteria

**AC-1: Device and simulator libraries build**
Given the pinned toolchain on the owner Mac with Xcode
When the static library is built for the device and the simulator targets with locked dependencies
Then both builds exit 0 and the Swift package links each of them into an empty app target without errors

**AC-2: Swift calls the core**
Given the Swift package on the simulator
When the Swift smoke test creates the core and calls `BackupCore::status` without a server
Then it returns a status value and the test passes with no network access

**AC-3: Bindings cannot drift**
Given the committed Swift bindings
When the drift check regenerates them from the current Rust surface
Then an unchanged surface passes, and a seeded change to a `BackupCore` method fails the check

**AC-4: No GPL crate in the iOS binary**
Given the iOS library's dependency tree
When the licence check runs with the allowlist
Then it passes, and a seeded GPL dependency makes it fail

**AC-5: No vault code in v1.1**
Given the v1.1 iOS build
When its linked crates are listed
Then `vault-core` is absent

## Non-Functional Requirements

**Compatibility**
- One static library contains `mobile_backup` and sync-core; the same UniFFI version as the Android build (uniffi 0.32.2), from the workspace lockfile.

**Reliability**
- Build and check commands print their progress (no quiet flags); a run without the Mac is `NOT-RUN(H-IOS)`, never PASS.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Drift check on the real and on a seeded surface | Pass, then fail |
| AC-4 | Licence check on the real tree and a seeded GPL crate | Pass, then fail |
| AC-5 | Linked-crate list of the v1.1 build | No `vault-core` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Owner Mac, clean checkout | Device and simulator builds, package link | Both exit 0; links | Compatibility |
| AC-2 | Simulator, no network | Swift smoke call of `status` | Status returned | |

## Constraints

- UniFFI only; no flutter_rust_bridge (ADR-013, S4-4). Swift owns the UniFFI handle and talks to Flutter over a Method/EventChannel.
- The Rust sources are owned by E-27 (`mobile_backup`) and E-23 (sync-core); this task builds and packages them and changes neither.
- iOS ignores user-installed CAs; no trust-store bridge is built for iOS.

## Risks & Mitigation

**Risk 1: Library layout differs from Android**
- *Risk*: the E-27 device spike may settle a layout that does not fit a static library.
- *Mitigation*: this task depends on E-27/c01 and reuses E-27/c18's scripts; differences are recorded in the build notes.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AGPL App Store exception needs consent from all copyright holders, and `COPYING.iOS` wording is still the owner's (ADR-011) | Owner | open | Medium |
| Apple Developer account, signing and App Store review are owner items (R18) | Owner | open | Low |
| Shared mobile config keys are named `android.*` in 17; 18 §10 says they become `mobile.*` when 18 is planned; the rename is an E-27 change | Owner / E-27 | open | Low |
| How vault code is kept out of v1.0 and v1.1 builds is module-layout open point 6; AC-5 checks the result only | E-27, E-29 owners | open | Low |
