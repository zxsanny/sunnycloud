# Contract: wasm_exports — `sync-core-wasm` exports and the cross-implementation vector file

**Component**: 13_sync_core (crate `sync-core-wasm`, a wasm-bindgen build of `sync-core-proto`)
**Producer task**: E-23/c16 — AZ-3969 (`_docs/02_tasks/done/AZ-3969_sync_core_wasm_vectors.md`)
**Consumer tasks**: other batches: E-26 (16 web transfer Worker; pipeline AZ-3671), E-27 (vector file in the UniFFI leg), E-04 (block vectors of `sc_ids` stay the base cases); resolved at Step 4
**Version**: 1.1.0
**Status**: implemented
**Last Updated**: 2026-11-13

## Purpose

The browser Worker drives `fetch` itself and calls this module for every rule that must match the server byte for byte: block hashing, rollup, commit ID and the resume diff of the upload planner (16 §Boundary). The vector file proves that native, WASM and UniFFI builds give identical results, so the web app carries no third protocol implementation.

Sources: `components/13_sync_core/description.md` §1, §2a, §7, §9; `components/16_web_app/description.md` (Boundary with 13, upload engine); `contracts/sync-core/crate_skeleton.md`; `contracts/shared_ids_hashing/sc_ids.md`; E-23 Testing Strategy (conformance).

## Shape

Build: `wasm32-unknown-unknown`, wasm-bindgen `=0.2.130`, packaged with wasm-pack 0.15.x for `--target web`, single-threaded; loaded by the transfer Worker, never on the main thread.

| Export | Signature (JavaScript view) | Errors | Notes |
|---|---|---|---|
| `hash_block` | `(block: Uint8Array) -> Uint8Array` (32 bytes) | `BlockTooLarge` above 2 MiB | block-sized calls only (bounded Worker heap, AC-50) |
| `rollup` | `(block_hashes: Uint8Array) -> Uint8Array` (32 bytes; input is n x 32 bytes, empty = zero-length file) | `InvalidLength` if not a multiple of 32 | |
| `new_commit_id` | `(unix_ms: number, random: Uint8Array /* 10 bytes */) -> string` (UUIDv7) | `InvalidTime` (not whole ms in 0..2^48), `InvalidLength` | pure: the Worker passes `Date.now()` and `crypto.getRandomValues(new Uint8Array(10))`; decided 2026-11-13 (owner: time from the browser; random bytes also from the caller so the module stays pure and the vectors can check the id) |
| `client_key_for_mobile` | `(device_id: string, asset_id: string, file_hash: Uint8Array) -> string` | `InvalidUuid`, `InvalidLength` | the mobile `client_key` derivation, exported so the key cases are compared; the web itself does not use it |
| `block_size` | `() -> number` (2097152) | | a function: wasm-bindgen 0.2.130 exports no constants |
| `UploadPlanner` | class: `new(block_count, need_blocks: Uint32Array \| number[], lanes_max, retry_base_ms, retry_max_ms, max_attempts)`; `next_actions() -> Action[]`; `on_result(i, {kind, retry_after_ms?})`; `on_session(need_blocks)`; `on_commit({kind, list?})` | `InvalidResult` for a malformed result object | the same state machine as `sync-core-proto` (`sync_core_proto.md`); `Action` has getters `kind` (including `GiveUp`), `i`, `ms` (u32 milliseconds) and `toJSON`; the `.d.ts` carries `BlockResult` and `CommitResult` |

Every error is a JavaScript `Error` whose `name` is the error kind.

### Vector file (repository fixture, shared with 16 and 17)

| Part | Content |
|---|---|
| block cases | the `sc_ids` vectors (0 bytes, 1 byte, 2 MiB, 2 MiB + 1 byte rejected) |
| file cases | sizes 0, 1, 2 MiB - 1, 2 MiB, 2 MiB + 1, 6 MiB, 100 MiB from a stated deterministic byte generator (large files are generated, not stored): expected block hashes and file hash |
| key cases | `(device_id, asset_id, file_hash)` triples with the expected mobile `client_key` |
| planner cases | resume diffs: `(block_count, need_blocks)` with the expected first `next_actions` |

The file stays under 1 MB. As built: `src/sync-core-wasm/vectors/sync_core_vectors.txt` (line-based text, 30 cases, about 9 KB), written by `cargo run -p sync-core-wasm --example gen_vectors` from native `sync-core-proto` and the `uuid` builder only (regenerating gives a byte-identical file); the `sc_ids` block lines are copied verbatim and a test checks they stay unchanged; file bytes come from the `sc_ids` generator (byte i = i mod 251). Commit-ID cases (epoch, maximum 48-bit time) are included.

### Gate

`scripts/ci/check-wasm-vectors.sh [--vectors FILE] [--legs native,node,browsers,uniffi] [--require-browsers]`: dependency split, wasm-pack build, then one `LEG <name> PASS|FAIL|NOT-RUN(reason)` line per leg; exit 1 on any FAIL or a runner that reports nothing; a browser that cannot launch is `NOT-RUN(launch: ...)` and fails only with `--require-browsers`. The UniFFI leg prints `NOT-RUN(EMU)` until open point 2 is answered. `scripts/ci/tests/test-check-wasm-vectors.sh` seeds a one-bit difference and checks every leg fails and names the case. Not yet in a CI job (E-31).

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

1. Closed 2026-11-13: the module exports `new_commit_id(unix_ms, random)`; the pure UUIDv7 builder lives in the adapter (it may move to `sc_ids` later).
2. Which UniFFI entry point the Kotlin leg calls to hash the vector files is not stated (no `SyncApi` operation is a free function); see `uniffi_surface.md` open point 2.
3. Size of the module: trimming exports is a risk-register item (13 risk 7), not decided here.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
| 1.1.0 | 2026-11-13 | As built (AZ-3969): `new_commit_id`, `client_key_for_mobile`, `block_size()`, planner errors, vector file path and format, gate | batch 58 |
