# Contract: sync-core and sync-core-wasm crate skeleton (boundary and client-trait registry)

**Component**: 13 sync_core (crates `sync-core`, `sync-core-wasm`; `sync-core-proto` is created by E-23)
**Producer task**: E-00/c07 — `_docs/02_tasks/todo/B01-01_sync_core_skeletons.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: other batches: E-23 (fills both crates, adds `sync-core-proto`), E-24 (14 daemon), E-27/E-28 (`mobile_backup`), E-26 (loads the WASM module)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

Fix the boundary of the two client crates before any client code exists: what each crate is, what it may depend on, how the WASM build is produced, and where the six client traits live. AZ-3652 re-scoped this E-00 row: the protocol DTOs those traits need (`Cursor`, `ChangePage`, ...) live in `sync-core-proto`, which E-23 creates, so the traits themselves are declared by E-23; this contract records their home and names so no other crate (in particular `contracts`) declares them.

Sources: `components/13_sync_core/description.md` §1, §2, §2a; `module-layout.md` (13, `sync-core-proto`, `sync-core-wasm`, layering table, exception 3); ADR-001, ADR-004, ADR-015; AZ-3652 crate table.

## Shape

### Crates

| Crate | Kind | Builds for | May depend on | Must never depend on |
|---|---|---|---|---|
| `sync-core` | library (native client part) | native targets | `sync-core-proto` (added by E-23), helpers config, error (client feature), clock, ids_hashing, names; tokio and reqwest from E-23 | `contracts`, any server crate, `sync-core-wasm` |
| `sync-core-wasm` | library, wasm-bindgen adapter (`cdylib` + `rlib`) | `wasm32-unknown-unknown` (and native for its tests) | `sync-core-proto` (added by E-23), helper ids_hashing, `wasm-bindgen =0.2.130` | `sync-core`, tokio, reqwest, `contracts`, any server crate |

- `wasm-bindgen` appears in no other crate of the workspace.
- The WASM package is produced with wasm-pack 0.15.x as the module the web app (E-26) loads in its transfer Worker. In the skeleton it exports nothing beyond what wasm-bindgen generates.

### Client-trait registry (declared by E-23 in `sync-core`)

| Trait | Methods (13 §2) | Implemented by |
|---|---|---|
| `SyncApi` | the typed client API of 13 §2 (`hash_file`, `upload`, `stage_small`, `batch_commit`, `download`, `poll_changes`, tree operations, device grant, mobile code, `refresh`, `sign_out`, `cancel`, ...) | `HttpSyncClient` (E-23); fakes in tests |
| `CredentialProvider` | `bearer() -> Credential{access, refresh?}`, `on_rotated(access, refresh)` | 14 token store, 17 `TokenVault` bridge |
| `FileSource` | `len`, `read_at`, `identity() -> (size, mtime_ns)` | hosts (14, `mobile_backup`) |
| `DownloadSink` | `write_verified(offset, bytes)`, `finish(verified)` | hosts |
| `CheckpointStore` | `load`, `save`, `clear(local_key)` with `UploadCheckpoint{session_id, block_hashes, file_hash, commit_id, state}` | 14 LocalIndex, 17 checkpoint DB; `MemoryCheckpointStore` for tests |
| `ProgressSink` | `on_progress(done, total)` | hosts |

`BackupCore` belongs to `mobile_backup`; `DaemonControl` to `ipc_schema`.

## Invariants

- No client crate (`sync-core*`, `mobile_backup`, `vault-core`, desktop crates) depends on `contracts` or a server component (AZ-3652 crate-graph test).
- The dependency tree of `sync-core-wasm` for `wasm32-unknown-unknown` contains no tokio, no reqwest, no `mio` and not `sync-core`.
- The six client traits are declared once, in `sync-core`; nothing in `contracts` names them.

## Non-Goals

- Not covered: any trait body, DTO or behaviour of 13 (E-23); the UniFFI feature and derives (E-23); the web app's loader (E-26).

## Versioning Rules

- Adding a dependency to either crate that is on the "must never" list is a breaking change to this contract and needs the owner's approval (ADR-015, ADR-001).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| wasm-build | `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm` in `src/` | exits 0 | also part of AZ-3652 AC-5 |
| wasm-pack | wasm-pack 0.15.x build of `sync-core-wasm` | produces a loadable package | |
| wasm-dep-tree | dependency tree of `sync-core-wasm` for wasm32 | no tokio, reqwest, mio, `sync-core` | |
| bindgen-single | workspace manifests | `wasm-bindgen` only in `sync-core-wasm` | |

## Open points (flagged in the producer task)

1. The E-00 row title says "with client traits"; AZ-3652 moved the traits to E-23, because their types live in `sync-core-proto`. The E-00 Definition of Done line on ports covers `contracts` only.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
