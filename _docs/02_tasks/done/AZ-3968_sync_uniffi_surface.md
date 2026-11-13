# UniFFI derives, foreign-trait callbacks and platform-verifier init hook

**Task**: AZ-3968_sync_uniffi_surface
**Name**: UniFFI surface of the sync core
**Description**: The `uniffi` feature that lets `mobile_backup` expose 13's types and host traits to Kotlin and Swift, plus the Android trust-store initialisation entry point, with no effect on builds that leave the feature off
**Complexity**: 3 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3958_sync_typed_errors, AZ-3959_http_sync_client_connect, AZ-3960_sync_tree_operations, AZ-3961_sync_hash_file, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3964_verified_download, AZ-3965_feed_client, AZ-3966_token_lifecycle, AZ-3967_sync_host_traits_foundation
**Component**: 13_sync_core (feature `uniffi` of `sync-core` and `sync-core-proto`)
**Tracker**: AZ-3968
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — produced by this task.
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md`, `sync_api.md`, `host_traits.md` — the types and traits that get derives.

## Problem

One upload implementation must serve every mobile entry point (AC-52). The mobile library is assembled by 17 (`mobile_backup` plus 13, one library per ABI, UniFFI for Kotlin and Swift; ADR-013, S4-4). For that, 13's records, errors and host traits must be usable across the FFI boundary, and Android needs the system trust store initialised before the first request (13 §3, risk 4). UniFFI and the `syn` pin can clash in the lockfile (13 risk 6, R31). None of this exists; E-27 and E-28 cannot build their libraries without it.

## Outcome

- Feature `uniffi` on `sync-core` and `sync-core-proto`: UniFFI derives on every public DTO, outcome and error type; the five host traits implementable as foreign callbacks; the client handle usable across the boundary, with `cancel()` as the only cancellation.
- An initialisation entry point the Android host calls once per process before the first request.
- With the feature off, no UniFFI crate in any dependency tree; one `uniffi 0.32.2` in the lockfile.

## Scope

### Included
- Derives under the feature for the types of the three contracts above.
- Foreign-trait support for `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink`.
- The Android verifier initialisation entry point (mechanism decided with E-27/c01 and E-27/c12; recorded in the contract).
- Binding generation for Kotlin and Swift as a check (no shipped bindings).
- The conformance entry point for the vector gate's UniFFI leg, if E-27 does not provide one (contract open point 2).

### Excluded
- Assembling the mobile library, `BackupCore`, the Kotlin and Swift layers (E-27, E-28).
- The vector gate itself (E-23/c16).

## Acceptance Criteria

**AC-1: Feature off leaves no trace**
Given the workspace built without the feature (desktop and WASM targets)
When the dependency trees are listed
Then no UniFFI crate appears in `sync-core`, `sync-core-proto` or `sync-core-wasm`

**AC-2: Bindings generate for both languages (AC-52)**
Given the crates built with the feature
When Kotlin and Swift bindings are generated
Then both succeed without manual edits and every public type of the three contracts appears in them

**AC-3: Foreign callbacks behave like Rust ones**
Given a foreign-language test implementation of `FileSource` and `CheckpointStore`
When `hash_file` runs over the foreign source and a checkpoint round trip runs over the foreign store
Then the results equal those of the Rust implementations over the same data

**AC-4: The feature changes no behaviour**
Given the unit suites of `sync-core-proto` and `sync-core`
When they run with and without the feature
Then the results are identical

**AC-5: One UniFFI version**
Given the workspace lockfile
When it is checked
Then it holds exactly one `uniffi` version, 0.32.2, and the build resolves the `syn` pin without a second copy

**AC-6: Trust store initialised before the first request (Android)**
Given the initialisation entry point and an Android test host
When the host calls it once and then connects to a server with a publicly trusted certificate
Then the connection verifies against the system trust store; without the call, the client returns a typed error instead of connecting

## Non-Functional Requirements

**Compatibility**
- `uniffi 0.32.2`; rustls-platform-verifier 0.7.1.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Dependency trees without the feature | No UniFFI crate |
| AC-4 | Suites with and without the feature | Identical |
| AC-5 | Lockfile check | One version |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Feature on | Kotlin and Swift generation | Success; all types present | AC-52 |
| AC-3 | Foreign test callbacks | Hash and checkpoint round trip | Equal to Rust | IT-13-20 (UniFFI leg input) |
| AC-6 | Android emulator (EMU) | Init then connect | Verified; typed error without init | NOT-RUN(EMU) without the emulator |

## Runtime Completeness

- **Named capability**: UniFFI 0.32.2 bindings for Kotlin and Swift, foreign-trait callbacks, and the Android system trust store through rustls-platform-verifier 0.7.1.
- **Production code that must exist**: real derives on the public types, real foreign-callback dispatch, the real verifier initialisation on Android.
- **Allowed external stubs**: foreign test implementations of the host traits in the binding tests.
- **Unacceptable substitutes**: hand-written binding glue that bypasses the generated bindings, a trust-all verifier, a second uploader behind the FFI.

## Constraints

- UniFFI only, no flutter_rust_bridge (ADR-013, S4-4).
- The feature only adds derives and the init entry point; no behaviour differs (13 §1).

## Risks & Mitigation

**Risk 1: Public type added later without derives**
- *Risk*: a later change adds a type the bindings cannot carry.
- *Mitigation*: AC-2 runs in CI, so the binding generation fails on the PR that adds it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Mechanism of the Android verifier initialisation (JNI export or UniFFI call) is open (`uniffi_surface.md` open point 1) | E-23 / E-27 | resolved 2026-11-13 (owner): a UniFFI call | Low |
| The vector gate's UniFFI leg needs an entry point that hashes a foreign source without a server; owner of that entry point open (`uniffi_surface.md` open point 2) | E-23 / E-27 | open | Low |
| AC-6 needs the emulator harness of E-27/c19 for the device run; without it the leg is `NOT-RUN(EMU)` | E-27 | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/13_sync_core/uniffi_surface.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
