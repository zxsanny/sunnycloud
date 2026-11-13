# Contract: uniffi_surface — the `uniffi` feature of `sync-core` and `sync-core-proto`

**Component**: 13_sync_core (feature `uniffi` of `sync-core` and `sync-core-proto`)
**Producer task**: E-23/c15 — AZ-3968 (`_docs/02_tasks/todo/AZ-3968_sync_uniffi_surface.md`)
**Consumer tasks**: this batch: E-23/c16 (UniFFI leg of the vector gate), E-28/c03 (iOS static library); other batches: E-27 (`mobile_backup` cdylib, `BackupCore` UniFFI surface, `TlsTrustBridge`, native library CI); pipelines AZ-3672, AZ-3674; resolved at Step 4
**Version**: 1.2.0
**Status**: draft
**Last Updated**: 2026-11-13

## Purpose

`mobile_backup` (17) assembles the one mobile library per ABI with UniFFI for Kotlin and Swift (ADR-013, S4-4). For that, 13's public types and host traits must be usable across the FFI boundary. The feature `uniffi` only adds derives and foreign-trait support; 13 builds no mobile library itself and has no flutter_rust_bridge.

Sources: `components/13_sync_core/description.md` §1 (feature `uniffi`), §3 (transport, platform verifier), §12 risks 4 and 6; `components/17_android_app/description.md` (native library, `TlsTrustBridge`); `canonical-names.md` S4-4; ADR-013.

## Shape

| Item | Rule |
|---|---|
| Feature | `uniffi`, off by default; the desktop daemon and the WASM build never enable it |
| Version | `uniffi 0.32.2`, one version in the workspace lockfile; its macros bring `syn` 2.0.119 while the rest of the workspace uses `syn` 3: one copy per major, not a pin clash |
| Records and enums | proto: `ServerInfo`, `Cursor`, `ChangeItem`, `ChangePage`, `ChangeState`, `ResyncReason`, `NodeKind`, `NodeView`, `ListingItem`, `ListingPage`, `RevisionView`, `RevisionRef`, `UploadSession`, `StartOutcome`, `CommitOutcome`, `Staged`, `BatchEntry`, `BatchCommit`, `EntryResult`, `BatchResult`, `DeviceAuthorization`, `DevicePoll`, `DeviceToken`, `TokenType`, `DeviceRefusal`, `SessionTokens`, `ResumeHashState`, `PageRule`, `PageViolation`, `Action`, `BlockResult`, `CommitResult`, `RetryPolicy`; core: `SyncConfig`, `Limits`, `HashedFile`, `Verified`, `FileIdentity`, `UploadState`, `UploadCheckpoint`, `BatchRecord`, `Credential`, `BatchOutcome`, `BatchReport`, `PlannerInput`. Scaffolding `sync_core_proto` and `sync_core` (one namespace per crate; `mobile_backup` adds its own). Converters: `Uuid`, `Hash32`, `FileHash` as wire text; `JsonValue` (`client_meta`, `import_hint`) as JSON text; `PageToken` as text; `SecretString` (= `Secret<String>`) as a string |
| Errors | `UrlError`, `BatchIdError`, `HostError` carry their variants. `SyncError`, `ConnectError`, `BatchRunError` are flat errors (variant plus the fixed `Display` text): their fields (`TransportKind`, `io::ErrorKind`, `&'static str`) have no UniFFI form, so a host that needs a field (`UpgradeRequired.message`, `RateLimited.retry_after`) reads it in Rust (`mobile_backup`) |
| Foreign traits | `ProgressSink` is exported as is. `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore` use `io::Result`, `Bytes` and unit-struct errors, so each has a foreign twin (`ForeignCredentialProvider`, `ForeignFileSource`, `ForeignDownloadSink`, `ForeignCheckpointStore`) with `Vec<u8>`, `String` keys and one error enum `HostError{NotFound, PermissionDenied, Failed}`; an unexpected foreign exception is `Failed` (logged without its text). `sync_core::Foreign<T>` turns a twin into the Rust trait |
| Objects | `HttpSyncClient`, exported methods `cancel()` (the only cancellation) and `limits()`. `connect` and the async operations are not exported (UniFFI's `tokio` feature is off); `mobile_backup` calls them in Rust. Not on the surface: `SyncApi`, the Rust host traits, `MemoryCheckpointStore`, `StoreError`, `PersistFailed`, `CredentialUnavailable`, `UploadRequest`, `SmallFile`, `HashCheckpoint`, `BatchSink`, `SourceResolver`, `SkippedFile`, `Mapped`, the executor internals, `UploadPlanner`, `ResumableFileHasher`, `ServerUrl`, `BadCursor`, `FileHasher`, `HashError` |
| Secrets | `Secret` values cross the boundary only inside `Credential` and `SessionTokens`; their foreign string form is never logged by 13 |
| Platform verifier | Android: one exported JNI function the host calls with its `Context` before the first request (open point 1); other platforms need no initialisation |

| Generated bindings | Kotlin data classes and Swift structs print field values in `toString`/`description`: the Rust `***` redaction does not cross the boundary, so the mobile hosts (E-27, E-28) must never log `Credential`, `SessionTokens` or `DeviceToken` |

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
| bindings-check | `sync-core/tests/uniffi/bindings_check.sh` | Kotlin and Swift bindings generated; every exported name present or listed as not on the surface | uses vault-core's bindgen binary; not in CI yet |
| isolation | `sync-core/tests/uniffi_isolation.rs` | feature-off trees (native and wasm) hold no UniFFI crate; one `uniffi` 0.32.2 | |
| foreign-callback (Rust side) | `sync-core/tests/uniffi_callbacks.rs` | foreign twins behave like the Rust traits | Kotlin leg NOT-RUN until open point 2 |
| one-version | workspace lockfile | exactly one `uniffi` version, 0.32.2 | 13 risk 6 |

## Open points

1. Resolved 2026-11-13 (owner): an exported JNI function, not a UniFFI call (a UniFFI call cannot pass the Android `Context`). Symbol `Java_org_sunnycloud_backup_TlsTrustBridge_nativeInit` (Android only, feature `uniffi`, Android-only `jni` 0.22.4), Kotlin `package org.sunnycloud.backup; object TlsTrustBridge { @JvmStatic external fun nativeInit(context: android.content.Context): Boolean }`, called once per process after `System.loadLibrary` and before the first request; it calls rustls-platform-verifier 0.7.1 `init_with_env` inside jni's `with_env` (panics caught, logged, `false`), is idempotent once it succeeded and can be retried after a failure. Until it succeeds, `connect` answers `ConnectError::TrustStoreUninitialized`. The package name is the producer's choice (E-27 may change it with the symbol); the Android compile check and the device run are not run yet (no Android target or emulator here).
2. The vector gate's UniFFI leg needs an exported entry point that hashes a foreign `FileSource` without a server; whether `mobile_backup` exposes one or `sync-core` exports a conformance function under the feature is not stated (E-23/c15 with E-27).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
| 1.1.0 | 2026-11-13 | Converters, flat errors, foreign twin traits and `Foreign` adapter, client object with `cancel`/`limits`, surface exclusions; generated bindings print secrets; open point 1 mechanism decided, implementation blocked | AZ-3968 |
| 1.2.0 | 2026-11-13 | Android init: exported JNI function `TlsTrustBridge.nativeInit` (owner), `ConnectError::TrustStoreUninitialized` before init | AZ-3968 |
