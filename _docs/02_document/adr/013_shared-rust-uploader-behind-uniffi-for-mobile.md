# ADR-013: Shared Rust uploader behind UniFFI for mobile

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Camera backup must start from the system, not from the Flutter UI: a MediaStore content-URI trigger, a periodic catch-up job, app open, and a user-initiated data-transfer job for large first syncs. A WorkManager worker runs without a Flutter engine, so a Dart-FFI Rust bridge is not callable there (`00_research/raw/b_clients_media.md` finding B12). The block protocol (BLAKE3 per block, blocklist, commit, resume) already exists in Rust for the desktop daemon, and writing it again in Dart, Kotlin and Swift would mean several copies, each needing the AC-3/7/8 test suite. Draft 02 proposed UniFFI for Kotlin and Swift plus flutter_rust_bridge for Flutter; the owner dropped flutter_rust_bridge at Step 4 (S4-4).

- Acceptance criteria addressed: AC-52 (one upload implementation for every mobile entry point, background run starts within 10 s at most 150 MB, 50,000 assets survive 10 forced kills with at most 1% re-hashed), AC-9 (new photo uploaded within 60 s on a Pixel), AC-78 (foreground app and background worker uploading one asset yield one server file), AC-46 (reinstall uploads 0 bytes of media), AC-53 (limited photo access text), AC-3 (resume re-sends at most 10 MiB)
- Restrictions addressed: RESTRICT-15 (native WorkManager with MediaStore trigger and user-initiated data-transfer jobs; one Rust uploader exposed through UniFFI, Kotlin owns the handle; iOS in v1.1), RESTRICT-05 (Flutter with native Kotlin and Swift where Flutter cannot reach a platform feature), RESTRICT-27 (iOS background work is best effort), RESTRICT-33 (device token excluded from backups)
- Risks addressed: R31 (UniFFI, NDK, 16 KB page size and JNA version problems), R32 (OEM task killers and Play permission policy), R05 (mobile token refresh lifecycle)
- Research source: `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — UniFFI 0.32.2 plus flutter_rust_bridge 2.13.0 (Option comparison); `_docs/00_research/raw/b_clients_media.md` § B12

The decision is non-trivial because both UniFFI and flutter_rust_bridge can expose the same core, but running both in one library costs a build tool and a pinned-dependency clash, and Flutter does not need Rust to display status.

## Decision

We will keep Flutter for the UI only and put all upload work in one shared Rust uploader, `mobile_backup` over `sync-core`, which Kotlin (Android, v1.0) and Swift (iOS, v1.1) call through UniFFI 0.32.2 and own.

One library per ABI (`cdylib` on Android, static library on iOS) contains `mobile_backup`, `sync-core` and later `vault_core`. Kotlin is the only code that talks to Android APIs and the only holder of the UniFFI handle: `BackupWorker`, `MediaTriggerWorker`, `CatchUpWorker` (15 min, queries MediaStore by generation), `FirstSyncJob` (user-initiated data-transfer job on Android 14+, a user-started `dataSync` foreground service on Android 11 to 13) and `AppOpenKick` all call `run_backup`; none implements upload. Flutter never calls Rust: it receives `BackupStatus` over an EventChannel and sends commands over a MethodChannel. The core exposes `run_backup`, `status`, `cancel`, `exchange_code`, `refresh`, `sign_out` and `rebuild_checkpoint`; `AssetSource` is a Kotlin callback trait. The checkpoint DB lives in the core, so a forced kill re-hashes at most 1%.

Build constraints: minSdk 30 (MediaStore generation API), NDK r28 or newer for 16 KB page alignment with a CI alignment check, JNA 5.17.0 or newer, `rustls-platform-verifier` with a small Kotlin TLS bridge for private-CA servers, tokio runtime threads attached to the JVM, and cancellation through an explicit `cancel()` flag because UniFFI has no native cancellation. There is no flutter_rust_bridge.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Pure Dart uploader (as Immich and Ente do today) | Contradicts "one implementation, in Rust" (RESTRICT-15) and needs a second protocol copy beside the desktop daemon; cannot run in a WorkManager worker without a headless engine (`mve_round2.md` option D) |
| Headless FlutterEngine started from WorkManager | Starts the Dart VM per run; RSS undocumented against the 150 MB cap of AC-52, plus engine and foreground coordination code (`mve_round2.md` option C; Immich is the precedent) |
| Fully native apps (Kotlin and Swift without Flutter) | Duplicates every screen twice and drops the shared Flutter UI the owner chose in RESTRICT-05; the upload part is already native, so the saving is small |
| Kotlin Multiplatform shared uploader | A second protocol implementation next to Rust `sync-core`, so the AC-3/7/8 suite and BLAKE3 behaviour must be proved twice; no research evidence for it |
| UniFFI plus flutter_rust_bridge (draft 02) | Spike found the frb template lockfile pins `syn 2.0.39`, which breaks UniFFI 0.32.2 until updated, plus a second code generator and Cargokit (archived); Flutter only needs status, which the Method/EventChannel already carries (R31) |

## Consequences

### Positive

- One upload implementation for foreground, trigger, catch-up and first-sync paths, so AC-78 is a single-process lock inside Rust, not cross-language coordination.
- A WorkManager worker uploads without a Flutter engine; the same crate serves iOS in v1.1 without a protocol rewrite.
- Removing flutter_rust_bridge drops the `syn` pin clash and one build tool (R31, R15 simplification).
- Flutter stays thin and testable; Kotlin owns permissions, tokens and the TLS bridge.

### Negative

- Each platform needs hand-written glue: the Kotlin layer, the Method/EventChannel contract, TLS-trust initialization and the cancel flag.
- JNA brings its own 16 KB-aligned `libjnidispatch.so` to check, and UniFFI async has only partial Swift 6 `Sendable` support.
- Flutter cannot call Rust directly, so any new Rust feature that Dart needs requires a Kotlin and a Swift pass-through (including the v1.2 vault on mobile).
- OEM task killers (Xiaomi, Huawei) and Play permission policy are outside this design and keep R32 open.

### Neutral / Open

- The Android device spike S9 (Pixel, Samsung, Xiaomi; 16 KB alignment, 150 MB RSS, forced kills) has not run.
- iOS reuses `mobile_backup` with a Swift layer; system-owned background uploads need a sans-IO core and stay deferred (RESTRICT-15, D1).
- The v1.2 vault core joins the same library; how mobile Flutter pages reach it without a Dart bridge is not specified in the component docs.

## Evidence

- `_docs/02_document/canonical-names.md` § Owner decisions at Step 4 (S4-4)
- `_docs/02_document/architecture.md` § 2 Technology Stack (Mobile)
- `_docs/02_document/components/17_android_app/description.md` § 1 High-Level Overview; § 2 Internal Interfaces (BackupCore; Kotlin entry points; Flutter UI); § 5 Implementation Details (Native library); § 15 Risks
- `_docs/02_document/components/18_ios_app/description.md` § 1 High-Level Overview
- `_docs/02_document/components/13_sync_core/description.md` § 1 High-Level Overview
- `_docs/02_document/system-flows.md` § Flow F16: Android camera backup
- `_docs/02_document/risk_mitigations.md` § Risk Register (R05, R31, R32); § Architecture/Component Changes Applied
- `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — UniFFI 0.32.2 plus flutter_rust_bridge 2.13.0 (Option comparison; Findings that change the plan, item 9)
- `_docs/00_research/raw/b_clients_media.md` § Findings table (B12, B13)

## Notes

Re-evaluate the single-library choice if a UniFFI or JNA bump forces a rebuild of the whole library for an unrelated consumer, or if S9 shows RSS above 150 MB. Re-evaluate the headless engine only if the UniFFI build fails 16 KB alignment on a supported NDK.
