# sync-core-wasm adapter and cross-implementation vector gate

**Task**: AZ-3969_sync_core_wasm_vectors
**Name**: WASM adapter and vector gate
**Description**: The wasm-bindgen exports the web Worker needs (block hash, rollup, commit ID, planner resume diff) and the shared vector file that proves native, WASM and UniFFI builds agree byte for byte
**Complexity**: 3 points
**Dependencies**: AZ-3739_sync_core_skeletons, AZ-3766_sc_ids, AZ-3955_sync_core_proto, AZ-3957_upload_planner, AZ-3968_sync_uniffi_surface
**Component**: 13_sync_core (crate `sync-core-wasm`)
**Tracker**: AZ-3969
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/wasm_exports.md` — produced by this task.
- `_docs/02_document/contracts/sync-core/crate_skeleton.md` — crate boundary, wasm-pack packaging, dependency-split check (E-00/c07).
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — base block vectors (E-04/c01).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — the rules exported.
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — entry point for the UniFFI leg (E-23/c15).

## Problem

The browser cannot run the native HTTP client, so the transfer Worker drives `fetch` itself (16 §Boundary). Every rule that must match the server byte for byte must still come from the one Rust implementation, or the web app carries a third protocol copy and AC-7 breaks in one browser only. The E-00 skeleton packages an empty module; nothing is exported, and no gate compares native, WASM and mobile outputs on a shared corpus.

## Outcome

- Exports `hash_block`, `rollup`, the commit-ID export (shape decided here, contract open point 1), `UploadPlanner` and `BLOCK_SIZE`, packaged with wasm-pack for `--target web`.
- A vector file (block, file, key and planner cases; under 1 MB; large files generated deterministically) shared with 16 and 17.
- A CI gate that fails on any difference between native, WASM (Node, Chromium, Firefox, WebKit) and the UniFFI build.

## Scope

### Included
- The wasm-bindgen exports of the `wasm_exports` contract, block-sized calls only.
- The vector file, its generator for large cases, and the comparison runners for native and WASM.
- The UniFFI leg runner over the entry point from E-23/c15 (EMU tier).
- Recording the commit-ID export decision with the E-04 and E-26 owners.

### Excluded
- The Worker, `fetch`, IndexedDB and the Vite integration (16, E-26).
- Adding the browser runs to the PR gate wiring (E-31/c17 CI tiers) beyond the gate's own job definition.

## Acceptance Criteria

**AC-1: Exports package for the browser**
Given the pinned toolchain, `wasm-bindgen =0.2.130` and wasm-pack 0.15.x
When the crate is built and packaged for `--target web`
Then the package exposes the exports of the contract, and the dependency-split check of E-00/c07 still finds no tokio, reqwest, mio or `sync-core`

**AC-2: Vector file complete**
Given the vector file
When it is read by the runners
Then it holds the `sc_ids` block cases, file cases for 0, 1, 2 MiB - 1, 2 MiB, 2 MiB + 1, 6 MiB and 100 MiB, key cases and planner resume-diff cases, and its size is under 1 MB

**AC-3: Native equals WASM (AC-7)**
Given the vector file
When the native runner and the WASM runner in Node, Chromium, Firefox and WebKit compute every case
Then there are 0 differences in block hashes, file hashes, keys and planner actions

**AC-4: Block-sized calls only (AC-50)**
Given a 2 MiB + 1 byte input and a 100 MiB file hashed block by block through the exports
When `hash_block` is called with the oversize input and the module's memory is read after each file case
Then the oversize call throws `BlockTooLarge`, and module memory after the 100 MiB case exceeds that after the 6 MiB case by at most one block

**AC-5: UniFFI leg (AC-52)**
Given the Kotlin UniFFI build with the entry point of E-23/c15 and the emulator (EMU)
When the vector file runs through it
Then there are 0 differences; without the emulator the leg reports `NOT-RUN(EMU)`, never PASS

**AC-6: The gate fails on a seeded difference**
Given one expected hash in a copy of the vector file changed by one bit
When the gate runs on that copy
Then it fails and names the case

## Non-Functional Requirements

**Performance**
- The gate's native and Node legs run in under 5 minutes (IT-13-20 budget).

**Compatibility**
- Chromium, Firefox and WebKit as named in the E-23 Testing Strategy.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Vector file schema and size | Complete; < 1 MB |
| AC-6 | Gate on a seeded file | Fails, names the case |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Clean checkout | Build and package | Exports present; tree clean | |
| AC-3 | Vector file | Native vs WASM (Node, 3 browsers) | 0 differences | IT-13-20, FT-N-01 |
| AC-4 | Oversize and 100 MiB cases | Memory and refusal | Throws; bounded memory | AC-50 |
| AC-5 | EMU, Kotlin build | UniFFI leg | 0 differences or `NOT-RUN(EMU)` | IT-13-20 |

## Runtime Completeness

- **Named capability**: the wasm-bindgen module (`wasm-bindgen =0.2.130`, wasm-pack 0.15.x) that runs `sync-core-proto` in the browser Worker.
- **Production code that must exist**: exports that call `sync-core-proto` and `sc_ids` compiled to wasm32; the real `UploadPlanner` behind the planner export.
- **Allowed external stubs**: none for the module; the browsers and emulator are external runners.
- **Unacceptable substitutes**: a JavaScript re-implementation of hashing, rollup or the planner, or vector expectations generated by the module under test.

## Constraints

- `wasm-bindgen` exists only in this crate (E-00/c07 check).
- The vector file extends, and never contradicts, the `sc_ids` block vectors.

## Risks & Mitigation

**Risk 1: WebKit runner availability**
- *Risk*: the WebKit leg needs a runner that CI may not have.
- *Mitigation*: the leg reports `NOT-RUN` with the reason instead of passing; the browsers container is E-31/c16.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Commit-ID export shape: `sc_ids::new_id` needs a `Clock`; whether the module exports a pure UUIDv7 constructor or the Worker makes keys itself (`wasm_exports.md` open point 1) | E-23 / E-04 / E-26 | open | Low |
| The UniFFI leg needs an entry point that hashes a foreign source without a server (`uniffi_surface.md` open point 2) and the emulator harness of E-27/c19 | E-23 / E-27 | open | Low |
| Trimming the WASM exports is a risk-register item (13 risk 7), not decided here | Owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/13_sync_core/wasm_exports.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
