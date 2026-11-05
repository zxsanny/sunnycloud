# Contract: uniffi_surface — the `uniffi` feature of `sync-core` and `sync-core-proto`

**Component**: 13_sync_core (feature `uniffi` of `sync-core` and `sync-core-proto`)
**Producer task**: E-23/c15 — `_docs/02_tasks/todo/B09-16_sync_uniffi_surface.md`; keys assigned at Step 4
**Consumer tasks**: this batch: E-23/c16 (UniFFI leg of the vector gate), E-28/c03 (iOS static library); other batches: E-27 (`mobile_backup` cdylib, `BackupCore` UniFFI surface, `TlsTrustBridge`, native library CI); pipelines AZ-3672, AZ-3674; resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

`mobile_backup` (17) assembles the one mobile library per ABI with UniFFI for Kotlin and Swift (ADR-013, S4-4). For that, 13's public types and host traits must be usable across the FFI boundary. The feature `uniffi` only adds derives and foreign-trait support; 13 builds no mobile library itself and has no flutter_rust_bridge.

Sources: `components/13_sync_core/description.md` §1 (feature `uniffi`), §3 (transport, platform verifier), §12 risks 4 and 6; `components/17_android_app/description.md` (native library, `TlsTrustBridge`); `canonical-names.md` S4-4; ADR-013.

## Shape

| Item | Rule |
|---|---|
| Feature | `uniffi`, off by default; the desktop daemon and the WASM build never enable it |
| Version | `uniffi 0.32.2`, one version in the workspace lockfile (13 risk 6: the `syn` pin is resolved there) |
| Records and enums | every public DTO of `sync_core_proto.md` and every outcome type of `sync_api.md` derives the UniFFI record or enum form under the feature |
| Errors | `ConnectError` and the typed operation errors of `sync_api.md` derive the UniFFI error form; token values never appear in an error |
| Foreign traits | `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink` (`host_traits.md`) can be implemented in Kotlin or Swift as foreign callbacks |
| Objects | the client handle can be held across the boundary; `cancel()` is the only cancellation (UniFFI has no async cancellation) |
| Secrets | `Secret` values cross the boundary only inside `Credential` and `SessionTokens`; their foreign string form is never logged by 13 |
| Platform verifier | an initialisation entry point the Android host calls once per process before the first request, so the system trust store is used (rustls-platform-verifier 0.7.1); not needed on iOS |

## Invariants

- With the feature off, no UniFFI crate is in the dependency tree of `sync-core`, `sync-core-proto` or `sync-core-wasm`.
- With the feature on, Kotlin and Swift bindings generate from the same sources without manual edits, and a change to a public type shows up as a binding diff (drift check of E-27/E-28).
- The feature changes no behaviour: tests run with and without it give the same results.

## Non-Goals

- Not covered: `BackupCore`, `AssetSource`, `TokenVault` and the library assembly (17, `mobile_backup`); the Swift package (E-28/c03).
- Not covered: Proguard rules and the Kotlin verifier component (E-27 `TlsTrustBridge`).

## Versioning Rules

- **Breaking**: a type removed from the FFI surface, a foreign-trait method changed, a UniFFI major or minor version change.
- **Non-breaking**: a new record field with a default, a new exported type.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| feature-off | dependency tree without the feature | no UniFFI crate | |
| bindings | Kotlin and Swift binding generation with the feature on | both succeed; no public type missing | |
| foreign-callback | a foreign `FileSource` and `CheckpointStore` driving `hash_file` and a store round trip | same results as the Rust implementations | |
| same-behaviour | unit suite with and without the feature | identical results | |
| one-version | workspace lockfile | exactly one `uniffi` version, 0.32.2 | 13 risk 6 |

## Open points

1. The mechanism of the Android verifier initialisation (an exported JNI function or a UniFFI call) is decided by E-23/c15 together with the E-27 device spike (E-27/c01) and `TlsTrustBridge` (E-27/c12).
2. The vector gate's UniFFI leg needs an exported entry point that hashes a foreign `FileSource` without a server; whether `mobile_backup` exposes one or `sync-core` exports a conformance function under the feature is not stated (E-23/c15 with E-27).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
