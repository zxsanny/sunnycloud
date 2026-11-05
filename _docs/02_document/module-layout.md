# Module Layout

**Language**: mixed (Rust Cargo workspace; Svelte 5 + TypeScript web app; Flutter + Kotlin + Swift mobile app)
**Layout Convention**: custom (one Cargo workspace under `src/`, web and mobile projects beside it; see ADR-driven exceptions)
**Root**: `src/`
**Last Updated**: 2026-11-04

Sources: E-00 § Interface specification (authoritative), `architecture.md`, `canonical-names.md`, `adr/`, `components/*/description.md` (Ports), `common-helpers/`, `data_model.md` §2, §13. Epic keys from `epics.md`. Crate names marked "proposed" are confirmed in E-00's first task.

## Layout Rules

1. Each component owns the directories listed in its entry: one in most cases; the multi-directory owners are listed under the exceptions.
2. Helpers live under `src/shared/<concern>/` as crates `sc_<concern>` (`sc_ids` for ids_hashing).
3. Cross-cutting concerns (config, errors, time, ids, names, database, audit, rate limit, logs and metrics) each have one entry under Shared / Cross-Cutting; component tasks use them and never re-implement them.
4. Public API of a Rust crate = its `src/lib.rs` re-exports (and the public modules named in the entry). Everything else is internal. A server component crate (layer 3) and 11 are imported only by the composition root 12; other server components reach them through `contracts` traits.
5. Tests: Rust unit tests in-module, integration tests in `<crate>/tests/`; web tests inside `src/web/`; Flutter tests in `src/mobile/test/`; system (black-box) tests in the repository-root `tests/` (E-31). All are inside the owner's `Owns` glob.
6. A `contracts` change is its own task in the providing component's epic. It edits only that provider's part of `src/contracts/`, runs alone in its batch, and never edits an implementer's directory in the same task.

## Per-Component Mapping

### Component: 01_identity
- **Epic**: AZ-3646 (E-15) · **Directory**: `src/components/01_identity/` (library `sunnycloud-identity`, proposed)
- **Public API**: `src/components/01_identity/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/01_identity/**`, `src/migrations/*_identity_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, audit, ids_hashing, db, ratelimit. Ports used (wired by 12): `AccountLifecycleHook` list (`initialize`)
- **Consumed by** (ports): 11 (`Authenticator`, `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService`), 02 (`AccountGuard`), 05 (`PrincipalRevalidator`, passed in by 11), 07 (`AccountAdmin`), 12 (`OperatorIdentityOps`, `InviteService`, `Janitor`)
> See ADR 015_ports-only-component-dependencies, ADR 008_built-in-oidc-sign-in-and-device-grant

### Component: 02_tree
- **Epic**: AZ-3640 (E-13) · **Directory**: `src/components/02_tree/` (library `sunnycloud-tree`, proposed)
- **Public API**: `src/components/02_tree/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/02_tree/**`, `src/migrations/*_tree_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, ids_hashing, names, db. Ports used: `AccountGuard` (01), `BlobCatalog`, `GcQueue`, `BlobRegistry` (04), `QuotaLedger` (07a), `FeedStamp`, `FeedNotifier`, `FeedHorizon` (05), `ThumbnailStateSource` (08), `CommitEffect` (supplied per call by 03 and 08)
- **Consumed by** (ports): 03 (`TreeCommit`, `TreeReader`), 06, 07, 08, 20 (`TreeReader`), 11 (`TreeService`, `TreeReader`), 12 (`TreeRestore`, `LiveUsageSource`, `TreeMaintenance`, `Janitor`), 04 (`BlobOwnerCleanup`), 01 and 07 (`AccountLifecycleHook`)
> See ADR 015_ports-only-component-dependencies (02 owns the commit transaction)

### Component: 03_transfer
- **Epic**: AZ-3643 (E-14) · **Directory**: `src/components/03_transfer/` (library `sunnycloud-transfer`, proposed)
- **Public API**: `src/components/03_transfer/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/03_transfer/**`, `src/migrations/*_transfer_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, ids_hashing, names, db. Ports used: `TreeCommit`, `TreeReader` (02), `QuotaLedger`, `QuotaAdmin::view` (07a), `BlobWriter`, `BlobReader`, `BlobRegistry`, `BlobCatalog`, `GcQueue` (04), `CommitEffect` from 08 (injected)
- **Consumed by** (ports): 11 (`UploadService`, `CommitService`, `DownloadService`), 02 (`CommitEffect`), 07 and 12 (`ReservationSource`), 12 (`Janitor`), 01 and 07 (`AccountLifecycleHook`)
> See ADR 015_ports-only-component-dependencies, ADR 004_blocklist-sync-over-http2-with-server-ordered-feed

### Component: 04_blobstore
- **Epic**: AZ-3627 (E-10) · **Directory**: `src/components/04_blobstore/` (library `sunnycloud-blobstore`, proposed)
- **Public API**: `src/components/04_blobstore/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/04_blobstore/**`, `src/migrations/*_blob_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, audit, ids_hashing, db. Ports used: `BlobOwnerCleanup` list (02, 08)
- **Consumed by** (ports): 03 and 08 (`BlobWriter`, `BlobReader`, `BlobRegistry`, `BlobCatalog`), 02 and 08 (`GcQueue`), 05 and 12 (`EpochMarker`), 12 (`Scrubber`, `BlobInventory`, `MasterKeyring`, `Janitor`), 07 and 12 (`DoctorCheck`, `HealthContributor`)
> See ADR 006_application-level-envelope-encryption-of-blobs, ADR 003_postgresql-metadata-with-plain-file-content

### Component: 05_sync_feed
- **Epic**: AZ-3635 (E-12) · **Directory**: `src/components/05_sync_feed/` (library `sunnycloud-sync-feed`, proposed)
- **Public API**: `src/components/05_sync_feed/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/05_sync_feed/**`, `src/migrations/*_feed_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, db. Ports used: `EpochMarker` (04); `PrincipalRevalidator` (01) arrives as a call parameter
- **Consumed by** (ports): 02 (`FeedStamp`, `FeedNotifier`, `FeedHorizon`), 11 (`ChangeFeed`), 12 (`EpochAdmin`, `DoctorCheck`), 01 and 07 (`AccountLifecycleHook`)
> See ADR 004_blocklist-sync-over-http2-with-server-ordered-feed, ADR 015_ports-only-component-dependencies

### Component: 06_sharing
- **Epic**: AZ-3619 (E-16) · **Directory**: `src/components/06_sharing/` (library `sunnycloud-sharing`, proposed)
- **Public API**: `src/components/06_sharing/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/06_sharing/**`, `src/migrations/*_share_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, audit, ids_hashing, ratelimit, db. Ports used: `TreeReader` (02)
- **Consumed by** (ports): 11 (`ShareService`, `PublicShareService`), 01 and 07 (`AccountLifecycleHook`)
> See ADR 015_ports-only-component-dependencies

### Component: 07_admin
- **Epic**: AZ-3624 (E-17) · **Directory**: `src/components/07_admin/` (library `sunnycloud-admin`, proposed); owns no table, so no migrations
- **Public API**: `src/components/07_admin/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/07_admin/**`
- **Imports from**: `contracts`; helpers config, error, clock, audit, db. Ports used: `AccountAdmin` (01), `AccountLifecycleHook` list (06, 20, 02, 03, 08, then 05, 07a; 20 before 02, owner decision 2026-11-05), `QuotaAdmin` (07a), `TreeReader` (02), `HealthContributor` list
- **Consumed by** (ports): 11 (`AdminService`, `AccountService`, `AccountDeletionService`), 12 (`AdminService`, `AccountDeletionService`, `DevSeed`, `Janitor` `DeletionPurger`)
> See ADR 015_ports-only-component-dependencies (07 drives erase; only 01 triggers hooks)

### Component: 07a_quota
- **Epic**: AZ-3631 (E-11) · **Directory**: `src/components/07a_quota/` (library `sunnycloud-quota`, proposed)
- **Public API**: `src/components/07a_quota/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/07a_quota/**`, `src/migrations/*_quota_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, db. Ports used: none (callers of `recompute` pass the numbers in)
- **Consumed by** (ports): 02 and 03 (`QuotaLedger`), 07, 11 and 12 (`QuotaAdmin`), 01 and 07 (`AccountLifecycleHook`)
> See ADR 015_ports-only-component-dependencies

### Component: 08_media_jobs
- **Epic**: AZ-3629 (E-18) · **Directory**: `src/components/08_media_jobs/` (library `media-api` in the 08 spec and E-18; `sunnycloud-media-jobs` by the E-00 pattern; see Open layout points)
- **Public API**: `src/components/08_media_jobs/src/lib.rs` · **Internal**: every other file, including the `JobQueue` SQL repository
- **Owns**: `src/components/08_media_jobs/**`, `src/migrations/*_job_*.sql`, and in `media-shared` everything except `decoder_protocol` (see Crate: media-shared)
- **Imports from**: `contracts`, `media-shared`; helpers config, error, clock, audit, db. Ports used: `TreeReader` (02), `BlobReader`, `BlobWriter`, `BlobRegistry`, `GcQueue`, `BlobCatalog` (04)
- **Consumed by** (ports): 02 via 03 (`CommitEffect`), 02 and 11 (`ThumbnailStateSource`), 11 (`DerivedOpener`, `WorkerAuthenticator`, `WorkerJobs`), 12 (`MediaAdmin`, `Janitor`, `DoctorCheck`), 04 (`BlobOwnerCleanup`), 07 (`AccountLifecycleHook`, `HealthContributor`); 09 over HTTP on `:8081` only
> See ADR 009_sandboxed-media-pipeline-with-api-only-workers (queue SQL lives here, in the API)

### Component: 09_worker_supervisor
- **Epic**: AZ-3639 (E-20) · **Directory**: `src/worker/` (binary `sunnycloud-worker`)
- **Public API**: none (binary; `src/main.rs`) · **Internal**: everything in the directory
- **Owns**: `src/worker/**`
- **Imports from**: `media-shared` (types, `policy`, `decoder_protocol` client side); helpers config, error, clock, ids_hashing. Never `contracts`, `sc_db`, `sqlx` or any component crate (crate-graph test)
- **Consumed by**: none (it calls 08's routes behind 11 over HTTP and the decoder socket)
> See ADR 009_sandboxed-media-pipeline-with-api-only-workers, ADR 010_cpu-first-transcoding-with-opt-in-gpu, ADR 001_rust-for-server-workers-and-desktop

### Component: 10_decoder
- **Epic**: AZ-3634 (E-19) · **Directory**: `src/decoder/` (binary `decoder-runner`)
- **Public API**: none (binary; its wire contract is `media-shared::decoder_protocol`) · **Internal**: `ImageExecutor`, `VideoExecutor`, `ProcessGuard`
- **Owns**: `src/decoder/**`, `src/media-shared/src/decoder_protocol.rs`, `src/media-shared/src/decoder_protocol/**`
- **Imports from**: `media-shared` (types and `decoder_protocol`, never `policy`); helpers config, error. No clock, no `sc_db`, no HTTP client, no `contracts`
- **Consumed by**: 09 only (Unix socket)
> See ADR 009_sandboxed-media-pipeline-with-api-only-workers, ADR 010_cpu-first-transcoding-with-opt-in-gpu, ADR 011_agpl-licence-with-app-store-exception

### Component: 11_http_api
- **Epic**: AZ-3644 (E-21) · **Directory**: `src/components/11_http_api/` (library `sunnycloud-http-api`, proposed)
- **Public API**: `src/components/11_http_api/src/lib.rs` (`HttpServer`, `ApiServices`, `HttpConfig`, `ServerInfo`, the route registry used by `openapi`) · **Internal**: listeners, routes, middleware, error mapper
- **Owns**: `src/components/11_http_api/**`
- **Imports from**: `contracts`; helpers config, error, clock, audit, ids_hashing, ratelimit, observability. No `sc_db`, no `sqlx`, no component crate. Ports used: every service in `ApiServices` (01, 02, 03, 05, 06, 07, 08, 20) and the `ReadinessCheck` list
- **Consumed by**: 12 (crate); clients 13, 16, 17, 18 and the supervisor 09 at runtime over HTTP
> See ADR 015_ports-only-component-dependencies (thin delivery: bind, one call, map), ADR 002_single-host-compose-with-one-api-process

### Component: 12_operator_cli (composition root)
- **Epic**: AZ-3647 (E-22) · **Directory**: `src/server/` (package `sunnycloud-server`, binary target `sunnycloud`)
- **Public API**: none (binary; nothing may import it) · **Internal**: `main.rs`, the clap command modules, `AppContext`, scheduler, janitor loops
- **Owns**: `src/server/**`
- **Imports from**: every server component crate (01-08, 07a, 20), 11 (`HttpServer`), `contracts`; helpers config, error, clock, audit, ids_hashing, names, db, ratelimit, observability. No `sqlx` queries, no blob paths (grep tests)
- **Consumed by**: none (operator, CI restore drill, Compose `migrate`/`serve` steps)
> See ADR 015_ports-only-component-dependencies (composition root, late hook and janitor lists), ADR 002_single-host-compose-with-one-api-process, ADR 014_two-database-roles-with-row-level-security

### Component: 13_sync_core (crate `sync-core`)
- **Epic**: AZ-3650 (E-23) · **Directory**: `src/sync-core/` (native only, tokio + reqwest executor)
- **Public API**: `src/sync-core/src/lib.rs` (`SyncApi`, `HttpSyncClient`, host traits `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink`, `MemoryCheckpointStore`) · **Internal**: executor, lanes, retry timing
- **Owns**: `src/sync-core/**` (13 also owns the two crate entries below)
- **Imports from**: `sync-core-proto`; helpers config, error (client feature), clock, ids_hashing, names. Never `contracts` or a server crate
- **Consumed by**: 14, `mobile_backup` (17, 18)
> See ADR 015_ports-only-component-dependencies (client traits live here), ADR 004_blocklist-sync-over-http2-with-server-ordered-feed

### Crate: sync-core-proto (component 13)
- **Epic**: AZ-3650 (E-23) · **Directory**: `src/sync-core-proto/` (sans-IO; builds native and `wasm32-unknown-unknown`). Not in E-00's list; created by E-23's first enabler (see Open layout points)
- **Public API**: `src/sync-core-proto/src/lib.rs` (protocol DTOs, `UploadPlanner`, commit-ID rules, feed-page checks, `ServerUrl`) · **Internal**: everything else
- **Owns**: `src/sync-core-proto/**`
- **Imports from**: helpers ids_hashing, error (client feature), names. No tokio, no HTTP, no clock
- **Consumed by**: `sync-core`, `sync-core-wasm`
> See ADR 004_blocklist-sync-over-http2-with-server-ordered-feed, ADR 001_rust-for-server-workers-and-desktop

### Crate: sync-core-wasm (component 13)
- **Epic**: AZ-3650 (E-23) · **Directory**: `src/sync-core-wasm/` (thin wasm-bindgen adapter; `wasm-bindgen =0.2.130` only here)
- **Public API**: `src/sync-core-wasm/src/lib.rs` (wasm-bindgen exports: block hash, rollup, commit ID, resume diff) · **Internal**: glue code
- **Owns**: `src/sync-core-wasm/**`
- **Imports from**: `sync-core-proto`; helper ids_hashing
- **Consumed by**: 16 (built module, loaded in the transfer Worker)
> See ADR 001_rust-for-server-workers-and-desktop (one core, WASM for Svelte), ADR 004_blocklist-sync-over-http2-with-server-ordered-feed

### Component: 14_desktop_daemon
- **Epic**: AZ-3625 (E-24) · **Directory**: `src/desktop/daemon/` (binary; package name not fixed in the docs)
- **Public API**: none (binary); its control surface is `DaemonControl` in `ipc_schema` · **Internal**: `FeedLoop`, `PairWorker`, `LocalIndexStore`, `PlatformFs`, `SafeFs`, `TokenStore`, tray, updater
- **Owns**: `src/desktop/daemon/**` (14 also owns the `ipc_schema` entry below)
- **Imports from**: `sync-core`, `ipc_schema`; helpers config, error, clock, ids_hashing, names
- **Consumed by**: 15 over JSON-RPC only (no crate import of the daemon)
> See ADR 012_native-desktop-daemon-with-separate-slint-settings, ADR 017_release-signing-and-offline-updater-keys

### Crate: ipc_schema (component 14)
- **Epic**: AZ-3625 (E-24) · **Directory**: `src/desktop/ipc_schema/`
- **Public API**: `src/desktop/ipc_schema/src/lib.rs` (`DaemonControl`, request and response types, method names, errors; no method returns a token) · **Internal**: none beyond serde glue
- **Owns**: `src/desktop/ipc_schema/**`
- **Imports from**: helper error only if wire codes are needed; no `sync-core`, no daemon internals
- **Consumed by**: 14, 15
> See ADR 012_native-desktop-daemon-with-separate-slint-settings, ADR 015_ports-only-component-dependencies

### Component: 15_desktop_settings
- **Epic**: AZ-3628 (E-25) · **Directory**: `src/desktop/settings/` (binary `sunnycloud-settings`)
- **Public API**: none (binary) · **Internal**: everything (Slint UI, `rfd` pickers, JSON-RPC client)
- **Owns**: `src/desktop/settings/**`
- **Imports from**: `ipc_schema`; helper config (IPC endpoint path rule). Never the daemon crate, `sync-core` or an HTTP client
- **Consumed by**: none (the user; spawned by the tray)
> See ADR 012_native-desktop-daemon-with-separate-slint-settings, ADR 011_agpl-licence-with-app-store-exception (Slint GPL option)

### Component: 16_web_app
- **Epic**: AZ-3632 (E-26) · **Directory**: `src/web/` (Svelte 5 + Vite + TypeScript SPA; scaffold created by E-26, not E-00)
- **Public API** (to its own pages; paths proposed, confirmed by the E-26 scaffold): `src/web/src/lib/transfer/index.ts` (`TransferService`), `src/web/src/lib/api/index.ts` (generated `ApiClient` and wrapper) · **Internal**: the transfer Worker and its protocol, WASM glue, pages, `renderMode`; pages import only the two facades
- **Owns**: `src/web/**`
- **Imports from**: 11's OpenAPI document (generates `ApiClient`), `sync-core-wasm` build output (13), `vault-core` WASM (19, v1.2 only)
- **Consumed by**: none (Caddy serves the built assets)
> See ADR 001_rust-for-server-workers-and-desktop, ADR 004_blocklist-sync-over-http2-with-server-ordered-feed (no third protocol implementation)

### Component: 17_android_app
- **Epic**: AZ-3637 (E-27) · **Directory**: `src/mobile/` (Flutter project; scaffold created by E-27): Dart UI in `lib/`, Kotlin layer in `android/`, tests in `test/`
- **Public API**: none (the native-to-Flutter Method/EventChannel contract is internal to 17 and 18) · **Internal**: Flutter UI, Kotlin workers (`BackupWorker`, `MediaTriggerWorker`, `CatchUpWorker`, `FirstSyncJob`, `AppOpenKick`), `AssetSource`, `TokenVault`, `TlsTrustBridge`
- **Owns**: `src/mobile/**` except `src/mobile/ios/**` (17 also owns the `mobile_backup` entry below)
- **Imports from**: `mobile_backup` through UniFFI, from Kotlin only; Dart never calls Rust
- **Consumed by**: 18 (shared Flutter `lib/`, read-only for 18, and `mobile_backup`)
> See ADR 013_shared-rust-uploader-behind-uniffi-for-mobile

### Crate: mobile_backup (component 17)
- **Epic**: AZ-3637 (E-27) · **Directory**: `src/mobile_backup/` (`cdylib` on Android, static library on iOS)
- **Public API**: `src/mobile_backup/src/lib.rs` (`BackupCore`, the `AssetSource` callback trait, UniFFI exports) · **Internal**: checkpoint DB, run mutex, asset-to-node mapping
- **Owns**: `src/mobile_backup/**`
- **Imports from**: `sync-core`; helpers config, error, ids_hashing, names; `vault-core` (v1.2 only)
- **Consumed by**: 17 (Kotlin), 18 (Swift)
> See ADR 013_shared-rust-uploader-behind-uniffi-for-mobile, ADR 015_ports-only-component-dependencies (`BackupCore` defined here)

### Component: 18_ios_app (v1.1)
- **Epic**: AZ-3641 (E-28) · **Directory**: `src/mobile/ios/` (Swift layer of the shared Flutter project)
- **Public API**: none · **Internal**: PhotoKit `AssetSource`, Keychain token store, background scheduling, Swift package for the UniFFI static library
- **Owns**: `src/mobile/ios/**`
- **Imports from**: `mobile_backup` through UniFFI, from Swift only; reads 17's `src/mobile/lib/`. Exception (decompose 2026-11-04): 18's iOS screens are added inside the shared Flutter shell owned by AZ-3999 (E-27/c15) by task AZ-3980 (E-28/c08); shared-crate changes for iOS go through AZ-4019 (E-27/c06b). Those two tasks must not run in the same batch as other E-27 edits of the same files.
- **Consumed by**: none (the user)
> See ADR 013_shared-rust-uploader-behind-uniffi-for-mobile, ADR 016_staged-releases-v1-0-v1-1-v1-2

### Component: 19_vault_core (crate `vault-core`, v1.2)
- **Epic**: AZ-3645 (E-29) · **Directory**: `src/vault-core/` (native, WASM and UniFFI builds)
- **Public API**: `src/vault-core/src/lib.rs` (vault create, unlock, rewrap, file encrypt and decrypt) · **Internal**: KDF, wrapping, header format
- **Owns**: `src/vault-core/**`
- **Imports from**: no sunnycloud crate (third-party crypto crates only, exact pins)
- **Consumed by**: 16 (WASM), `mobile_backup` (17, 18); never a server crate
> See ADR 007_opt-in-client-side-vault, ADR 016_staged-releases-v1-0-v1-1-v1-2

### Component: 20_vault_service (v1.2)
- **Epic**: AZ-3648 (E-30) · **Directory**: `src/components/20_vault_service/` (library `sunnycloud-vault-service`, proposed)
- **Public API**: `src/components/20_vault_service/src/lib.rs` · **Internal**: every other file in the directory
- **Owns**: `src/components/20_vault_service/**`, `src/migrations/*_vault_*.sql`
- **Imports from**: `contracts`; helpers config, error, clock, audit, db. Never `vault-core`. Ports used: `TreeReader` (02)
- **Consumed by** (ports): 11 (`VaultService`), 07 (`AccountLifecycleHook`)
> See ADR 007_opt-in-client-side-vault, ADR 016_staged-releases-v1-0-v1-1-v1-2, ADR 015_ports-only-component-dependencies

### Crate: contracts (server ports)
- **Epic**: AZ-3621 (E-00) · **Directory**: `src/contracts/`
- **Public API**: `src/contracts/src/lib.rs` (every port trait and shared DTO of `canonical-names.md` § Ports and E-00's list; signatures only)
- **Owns**: `src/contracts/**`: E-00. Later signature changes follow Layout Rule 6
- **Imports from**: layer-1 helper types only (`sc_db` for `DbTx` alone; `sc_observability` for `HealthContributor` and `ReadinessCheck`, re-exported, owner decision 2026-11-05)
- **Consumed by**: 01-08, 07a, 11, 12, 20; never 09, 10, 13-19 (server-only)
> See ADR 015_ports-only-component-dependencies

### Crate: media-shared
- **Epic**: AZ-3629 (E-18) for the crate, `types` and `policy`; AZ-3634 (E-19) for `decoder_protocol`; skeleton by E-00
- **Directory**: `src/media-shared/` · **Public API**: `src/media-shared/src/lib.rs` with the public modules `types`, `policy`, `decoder_protocol`
- **Owns**: `src/media-shared/**` except the `decoder_protocol` module: 08. `decoder_protocol`: 10 (see 10's entry)
- **Imports from**: serde-level crates only: no `sqlx`, no I/O, no `contracts`, no component (crate-graph test)
- **Consumed by**: 08, 09, 10 (10 never calls `policy`)
> See ADR 009_sandboxed-media-pipeline-with-api-only-workers, ADR 010_cpu-first-transcoding-with-opt-in-gpu

## Shared / Cross-Cutting

All are library crates with the public API in `src/lib.rs`, created empty by E-00 and filled by their epic.

### shared/config
- **Directory**: `src/shared/config/` (`sc_config`) · **Owned by**: AZ-3623 (E-01)
- **Purpose**: typed, validated settings; `SUNNYCLOUD_*` key table; `Secret<T>`; client reload
- **Consumed by**: 01-12, 13, 14, 15 (endpoint path only), `mobile_backup`

### shared/error
- **Directory**: `src/shared/error/` (`sc_error`; features `server`, `client`) · **Owned by**: AZ-3626 (E-02)
- **Purpose**: one failure taxonomy, wire codes, the status table used only by 11's mapper, client parser
- **Consumed by**: all server crates, 09, 10, `sync-core-proto`, `sync-core`, 14, `mobile_backup`

### shared/clock
- **Directory**: `src/shared/clock/` (`sc_clock`) · **Owned by**: AZ-3630 (E-03)
- **Purpose**: injectable `Clock` (system, offset for the test image, manual for tests)
- **Consumed by**: server components, `sc_ids`, `sc_names`, 09, 13, 14

### shared/ids_hashing
- **Directory**: `src/shared/ids_hashing/` (`sc_ids`; wasm32 build for the hashing part) · **Owned by**: AZ-3633 (E-04)
- **Purpose**: ids, BLAKE3 block and file hashes, rollup, commit keys, tokens, constant-time compare
- **Consumed by**: 01-04, 06, 11, 12, 09, `sync-core-proto`, `sync-core`, `sync-core-wasm`, 14, `mobile_backup`

### shared/names
- **Directory**: `src/shared/names/` (`sc_names`; pure, WASM-buildable) · **Owned by**: AZ-3636 (E-05)
- **Purpose**: legal names, `name_key`, conflict-copy names, client portability and escaping
- **Consumed by**: 02, 03, 12, `sync-core-proto`, `sync-core`, 14, `mobile_backup`

### shared/db
- **Directory**: `src/shared/db/` (`sc_db`) · **Owned by**: AZ-3638 (E-06)
- **Purpose**: pool, short transactions, `DbTx`, account and system scope (the only place `sunnycloud.scope` appears), role checks, `Migrator` embedding `src/migrations/`
- **Consumed by**: 01-08, 07a, 12, 20, `sc_audit`, `sc_observability`; `contracts` (`DbTx` only). Never 09, 10, 11 or clients
> See ADR 014_two-database-roles-with-row-level-security

### shared/audit
- **Directory**: `src/shared/audit/` (`sc_audit`, server only) · **Owned by**: AZ-3642 (E-07)
- **Purpose**: append-only, IDs-only `audit_log` writer, reader for export, retention purge call
- **Consumed by**: 01, 04, 06, 07, 07a, 11, 12, 20

### shared/ratelimit
- **Directory**: `src/shared/ratelimit/` (`sc_ratelimit`, server only) · **Owned by**: AZ-3620 (E-08)
- **Purpose**: in-memory `AbuseLimiter` for the AC-20 failures; constructed once by 12
- **Consumed by**: 01, 06, 11 (maps to 429), 12

### shared/observability
- **Directory**: `src/shared/observability/` (`sc_observability`, name proposed by E-09) · **Owned by**: AZ-3622 (E-09)
- **Purpose**: JSON logs, Prometheus registry and collectors, the ops router (`/healthz`, `/readyz`, `/metrics`)
- **Consumed by**: 11, 12, `contracts` (re-exports `HealthContributor` and `ReadinessCheck`, which this crate declares; owner decision 2026-11-05) (and 09 for its own ops port if E-20 chooses it)

## Migrations (`src/migrations/`)

The directory, the committed `src/.sqlx/` data and the `cargo sqlx prepare --check` gate come from E-00; the `Migrator` that embeds the files is E-06. Each reversible pair `YYYYMMDDHHMMSS_<verb>_<object>.up.sql` / `.down.sql` (data_model §13.1) belongs to the component that owns the table (data_model §2). The `<object>` carries exactly one domain word followed by at least one more word (`create_quota_table`), so the owner glob `*_<word>_*.sql` matches it; grants to `sunnycloud` go in the same file as the table.

| Domain word | Owner | Epic | Tables |
|---|---|---|---|
| `accounts_base` | shared/db (E-06) | AZ-3638 | minimal `accounts` (id, state, created_at), first migration (owner decision 2026-11-04) |
| `identity` | 01 | AZ-3646 | identity columns of `accounts`; identities, sessions, device_tokens, device_grants, invites, oauth_states, login_codes |
| `tree` | 02 | AZ-3640 | nodes (all columns except `seq`), revisions |
| `transfer` | 03 | AZ-3643 | upload_sessions, commit_receipts |
| `blob` | 04 | AZ-3627 | blobs, blob_gc |
| `feed` | 05 | AZ-3635 | account_feeds; adds `nodes.seq` and `nodes_feed_idx` to 02's table (only these) |
| `share` | 06 | AZ-3619 | share_links |
| `quota` | 07a | AZ-3631 | quota |
| `job` | 08 | AZ-3629 | jobs, derived, worker_status |
| `audit` | helper audit | AZ-3642 | audit_log and its `SECURITY DEFINER` purge function |
| `vault` | 20 | AZ-3648 | vaults (v1.2, additive) |

> See ADR 003_postgresql-metadata-with-plain-file-content, ADR 014_two-database-roles-with-row-level-security

## Repository-level paths (outside the component tree)

| Path | Owner | Rule |
|---|---|---|
| `src/Cargo.toml`, `src/rust-toolchain.toml`, `src/rustfmt.toml`, `src/deny.toml`, `src/.cargo/` | AZ-3621 (E-00) | Later edits only where an epic names them: E-23 adds the `sync-core-proto` member, E-15 adds the `jsonwebtoken` pin |
| `src/Cargo.lock`, `src/.sqlx/` | generated, no owner | Regenerated by the task that changes a dependency or a query, never hand-edited; two tasks that change them never run in parallel |
| `tests/` (harness, `run.sh`, `compose.test.yml`, `e2e-ctl`, data sets, lab) | AZ-3649 (E-31); skeleton by E-00 | Component-level tests stay in the component's own directory |
| `deploy/` (`docker/`, `postgres/`, compose files), `.dockerignore` | AZ-3621 (E-00) | Later changes by the epic whose AC needs them, one task at a time |
| `.github/workflows/`, `scripts/ci/` | AZ-3621 (E-00) for `ci.yml`, `audit.sh`, `watchdog.sh` | Release workflows are outside E-00; signing only from GitHub |
| `LICENSE`, `COPYING.iOS`, `.gitignore`, `README.md` | AZ-3621 (E-00) | `COPYING.iOS` waits for the owner's wording |

> See ADR 011_agpl-licence-with-app-store-exception, ADR 017_release-signing-and-offline-updater-keys

## Allowed Dependencies (layering)

Read top to bottom. A layer imports only what its row names. A lower layer never imports a higher one, and imports inside one layer are forbidden unless the row names them.

| Layer | Components | May import from |
|-------|------------|-----------------|
| 6. Entry points | 12 `server`, 09 `worker`, 10 `decoder`, 14 `desktop/daemon`, 15 `desktop/settings`, 16 `web`, 17 and 18 `mobile` | 12: layers 1-5 on the server side; it is the only crate that imports layer-3 crates and 11. 09: 1, `media-shared`. 10: 1 (config, error), `media-shared`. 14: 1, `ipc_schema`, `sync-core`. 15: 1 (config), `ipc_schema`. 16: `sync-core-wasm` output, `vault-core` WASM (v1.2), 11's OpenAPI. 17 and 18: `mobile_backup` through UniFFI |
| 5. Delivery and client adapters | 11 `http_api`, `mobile_backup` | 11: 1 and `contracts`, never layer 3 or 4. `mobile_backup`: 1, `sync-core`, `vault-core` (v1.2) |
| 4. Client cores | 13 `sync-core`, `sync-core-wasm` | 1, `sync-core-proto` |
| 3. Server domain components | 01, 02, 03, 04, 05, 06, 07, 07a, 08, 20 | 1, `contracts` (08 also `media-shared`); never another layer-3 crate |
| 2. Contracts and shared types | `contracts`, `media-shared`, `sync-core-proto`, `ipc_schema`, 19 `vault-core` | 1 only. `vault-core` imports nothing of sunnycloud |
| 1. Shared / Foundation | `src/shared/{config,error,clock,ids_hashing,names,db,audit,ratelimit,observability}` | other layer-1 helpers, acyclic: `sc_ids` and `sc_names` use `sc_clock`; `sc_db` uses `sc_config`, `sc_clock`, `sc_error`; `sc_audit` uses `sc_db`, `sc_clock`; `sc_ratelimit` uses `sc_config`, `sc_clock`; `sc_observability` uses `sc_config`, `sc_error`, `sc_clock`, `sc_db`, `sc_audit`, `sc_ratelimit` and never `contracts` (owner decision 2026-11-05) |

Violations of this table are **Architecture** findings in code-review Phase 7 and are High severity. The E-00 crate-graph test checks: no edge between crates under `src/components/`, nothing imports `src/server/`, no `sqlx` in 09, 10, 11, 12 (direct) or `media-shared`.

> See ADR 015_ports-only-component-dependencies

## ADR-driven exceptions to the conventional layout

1. **Root `src/`, not `crates/`.** The Rust convention puts workspace members under `crates/`. The owner rule (new projects keep source under `src/`) and E-00 § Interface specification place the workspace at `src/Cargo.toml`, server components at `src/components/<nn_name>/` (numeric id kept so folder and component spec match), helpers at `src/shared/<concern>/`, and binaries and type crates at `src/<name>/`. This is an owner rule, not an ADR conflict. One workspace for all Rust code is ADR-001's decision.
   > See ADR 001_rust-for-server-workers-and-desktop
2. **The composition root imports every component crate.** Rule 4 forbids imports between components; ADR-015 makes `sunnycloud-server` (12) the single exception. That is why 12 lives in `src/server/`, outside `src/components/`, and why no crate may import it.
   > See ADR 015_ports-only-component-dependencies
3. **Components with more than one directory.** 13 owns `sync-core`, `sync-core-proto` and `sync-core-wasm`: the client traits live in `sync-core`, and the WASM build must not pull tokio. 14 owns `ipc_schema`, so 15 depends on nothing else of 14. 17 owns `mobile_backup`, the one Rust uploader shared with 18. 08 and 10 split `media-shared` by module (`decoder_protocol` belongs to 10).
   > See ADR 015_ports-only-component-dependencies, ADR 012_native-desktop-daemon-with-separate-slint-settings, ADR 013_shared-rust-uploader-behind-uniffi-for-mobile, ADR 009_sandboxed-media-pipeline-with-api-only-workers
4. **Shared crates outside `src/shared/`.** `contracts`, `media-shared`, `sync-core-proto` and `ipc_schema` have owning components or epics and carry ports and types, not helpers, so they sit at `src/<name>/` as E-00 lists them. `contracts` is server-only and never a client dependency.
   > See ADR 015_ports-only-component-dependencies
5. **Two ports declared in a helper.** Resolved by owner decision 2026-11-05: `HealthContributor` and `ReadinessCheck` (with their items) are declared in `sc_observability` and re-exported by `contracts`; `sc_observability` imports no `contracts`. No helper imports `contracts`, so layer 1 stays below it.
6. **Vault crates are separate and not shipped in v1.0 or v1.1.** `vault-core` and `20_vault_service` are their own directories so v1.0 and v1.1 builds can leave them out. 12's import of 20 and `mobile_backup`'s import of `vault-core` must not be linked into v1.0 or v1.1 release builds; E-22, E-27, E-29 and E-30 decide how (open point 6). 20 never imports `vault-core`.
   > See ADR 016_staged-releases-v1-0-v1-1-v1-2, ADR 007_opt-in-client-side-vault

No Accepted ADR is contradicted, and none needs the supersede flow. ADR-005 (conflict rules) and ADR-008 (sign-in design) change behaviour only; their owners are 02 and 01.

## Open layout points (confirmed by the owner on 2026-11-04)

The owner approved this layout and the readings below on 2026-11-04; they are no longer open.

1. `sync-core-proto` is named by `components/13_sync_core/description.md` and E-23 but is missing from E-00's layout, which describes a "native/WASM feature split" instead. This layout follows 13 and E-23 (a separate `src/sync-core-proto/`, created by E-23).
2. 08's crate is `media-api` in `components/08_media_jobs/description.md` and E-18, but would be `sunnycloud-media-jobs` by E-00's `sunnycloud-<name>` pattern. The directory stays `src/components/08_media_jobs/` either way.
3. 12's package is `sunnycloud-server` (`architecture.md`, `deployment/containerization.md`, E-00 notes), while E-00 scope calls it "the `sunnycloud` binary crate". This layout reads both as package `sunnycloud-server` with binary target `sunnycloud`.
4. Helper 07 (`common-helpers/07_helper_db.md`) lists 11 as a `sc_db` user for readiness, but `components/11_http_api/description.md` lists no db helper. This layout follows 11: 12 registers a database `ReadinessCheck`.
5. Helper 05 lists 19 as a `sc_ids` user, but `components/19_vault_core/description.md` says 19 has no sunnycloud dependency, so that its WASM stays small and auditable. This layout follows 19.
6. `components/12_operator_cli/description.md` lists "helper 04" as a `Janitor` provider, which would make `sc_audit` import `contracts`. This layout keeps helpers free of `contracts` instead: 12 wraps `sc_audit`'s purge call in a `Janitor` adapter. The vault feature-gating mechanism (exception 6) is also open.
7. `vault-core` (E-00, ADR-007) and `vault_core` (ADR-001, ADR-013, ADR-016) name the same crate (Cargo treats `-` and `_` as the same). The directory is `src/vault-core/`.
8. E-00 lists `deploy/compose/`, while `deployment/containerization.md` and `environment_strategy.md` name `deploy/compose.yml`. Either way it is under `deploy/` and owned by E-00.

## Layout Conventions (reference)

| Language | Root | Per-component path | Public API file | Test path |
|----------|------|-------------------|-----------------|-----------|
| Rust (this repo) | `src/` | `src/components/<nn_name>/`, `src/shared/<concern>/`, `src/<crate>/` | `<crate>/src/lib.rs` | in-module unit tests, `<crate>/tests/` |
| TypeScript / Svelte 5 (16) | `src/web/` | `src/web/src/lib/<module>/` | `src/web/src/lib/<module>/index.ts` (barrel) | inside `src/web/` (Vitest, Playwright) |
| Dart / Flutter + Kotlin + Swift (17, 18) | `src/mobile/` | `lib/` (Dart), `android/` (Kotlin), `ios/` (Swift) | none (UniFFI boundary is `mobile_backup`) | `src/mobile/test/` |
