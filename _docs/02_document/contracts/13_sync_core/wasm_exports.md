# Contract: wasm_exports — `sync-core-wasm` exports and the cross-implementation vector file

**Component**: 13_sync_core (crate `sync-core-wasm`, a wasm-bindgen build of `sync-core-proto`)
**Producer task**: E-23/c16 — `_docs/02_tasks/todo/B09-17_sync_core_wasm_vectors.md`; keys assigned at Step 4
**Consumer tasks**: other batches: E-26 (16 web transfer Worker; pipeline AZ-3671), E-27 (vector file in the UniFFI leg), E-04 (block vectors of `sc_ids` stay the base cases); resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The browser Worker drives `fetch` itself and calls this module for every rule that must match the server byte for byte: block hashing, rollup, commit ID and the resume diff of the upload planner (16 §Boundary). The vector file proves that native, WASM and UniFFI builds give identical results, so the web app carries no third protocol implementation.

Sources: `components/13_sync_core/description.md` §1, §2a, §7, §9; `components/16_web_app/description.md` (Boundary with 13, upload engine); `contracts/sync-core/crate_skeleton.md`; `contracts/shared_ids_hashing/sc_ids.md`; E-23 Testing Strategy (conformance).

## Shape

Build: `wasm32-unknown-unknown`, wasm-bindgen `=0.2.130`, packaged with wasm-pack 0.15.x for `--target web`, single-threaded; loaded by the transfer Worker, never on the main thread.

| Export | Signature (JavaScript view) | Errors | Notes |
|---|---|---|---|
| `hash_block` | `(block: Uint8Array) -> Uint8Array` (32 bytes) | throws `BlockTooLarge` above 2 MiB | block-sized calls only (bounded Worker heap, AC-50) |
| `rollup` | `(block_hashes: Uint8Array) -> Uint8Array` (32 bytes; input is n x 32 bytes, empty = zero-length file) | throws `InvalidLength` if not a multiple of 32 | |
| commit ID | see open point 1 | | |
| `UploadPlanner` | class: `new(block_count, need_blocks: Uint32Array, lanes_max, retry_base_ms, retry_max_ms, max_attempts)`; `next_actions() -> Action[]`; `on_result(i, result)`; `on_session(need_blocks)`; `on_commit(result)` | none | the same state machine as `sync-core-proto` (`sync_core_proto.md`); `Action` and result values as tagged objects `{kind, ...}` |
| `BLOCK_SIZE` | number (2097152) | | |

### Vector file (repository fixture, shared with 16 and 17)

| Part | Content |
|---|---|
| block cases | the `sc_ids` vectors (0 bytes, 1 byte, 2 MiB, 2 MiB + 1 byte rejected) |
| file cases | sizes 0, 1, 2 MiB - 1, 2 MiB, 2 MiB + 1, 6 MiB, 100 MiB from a stated deterministic byte generator (large files are generated, not stored): expected block hashes and file hash |
| key cases | `(device_id, asset_id, file_hash)` triples with the expected mobile `client_key` |
| planner cases | resume diffs: `(block_count, need_blocks)` with the expected first `next_actions` |

The file stays under 1 MB.

## Invariants

- Every export gives the same bytes as `sync-core-proto` native for every vector case; the gate fails on any difference.
- The module's dependency tree has no tokio, reqwest, mio or `sync-core` (`crate_skeleton.md`).
- No export accepts more than one block of input bytes at a time.
- No export performs I/O, reads a clock or holds a token.

## Non-Goals

- Not covered: HTTP, IndexedDB resume records, the Worker protocol, `Blob.slice` reading (16).
- Not covered: the change-feed checker and the other `sync-core-proto` rules the web app does not use.

## Versioning Rules

- **Breaking**: an export removed or its argument layout changed; a vector expectation changed (that is a protocol change).
- **Non-breaking**: a new export; new vector cases.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| vectors-node | vector file in Node | 0 differences against native | IT-13-20 |
| vectors-browsers | vector file in Chromium, Firefox, WebKit | 0 differences | IT-13-20, E-23 Testing Strategy |
| oversize | 2 MiB + 1 byte to `hash_block` | `BlockTooLarge` | `sc_ids` oversize |
| planner-parity | resume-diff cases through WASM and native | identical action lists | IT-13-2 (a) |
| uniffi-leg | vector file through the Kotlin UniFFI build | 0 differences (EMU tier; `NOT-RUN(EMU)` without the emulator) | IT-13-20 |

## Open points

1. "Commit ID" export: the web needs a random UUIDv7 per upload and per batch (16 upload engine); `sc_ids::new_id` takes a `Clock` and has no pure form from (time, random bytes). E-23/c16 decides with the E-04 and E-26 owners whether the module exports such a constructor or the Worker generates UUIDv7 itself; the mobile derivation is not used by the web.
2. Which UniFFI entry point the Kotlin leg calls to hash the vector files is not stated (no `SyncApi` operation is a free function); see `uniffi_surface.md` open point 2.
3. Size of the module: trimming exports is a risk-register item (13 risk 7), not decided here.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
