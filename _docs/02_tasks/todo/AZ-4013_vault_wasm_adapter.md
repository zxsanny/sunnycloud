# WASM adapter for the vault core: module Worker exports and the 100 MiB browser probe [v1.2]

**Task**: AZ-4013_vault_wasm_adapter
**Name**: Vault WASM adapter
**Description**: Export the vault operations through `wasm-bindgen` for a single-threaded module Web Worker with block-sized calls and randomness from `crypto.getRandomValues`, and prove in the web app's build that a 100 MiB file encrypts and decrypts in a Worker with bounded memory
**Complexity**: 3 points
**Dependencies**: AZ-4008_passphrase_policy, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4012_lock_zeroize, AZ-3930_vite_wasm_spike
**Component**: 19 vault_core [v1.2] (WASM target)
**Tracker**: AZ-4013
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — "WASM form" (this task contributes it).
- Record of E-26/c01 (how the Vite build packages wasm-bindgen output for a module Worker under the strict CSP).
- `components/19_vault_core/description.md` §5; `acceptance_criteria.md` AC-14, AC-62.

## Problem

The web vault page (E-26/n01) runs the crypto in a module Web Worker, single-threaded, without COOP/COEP (19 §5). Memory on iOS Safari is tight: 100 MiB must open within 250 MB total (AC-14), so content must cross the boundary one chunk at a time. Randomness must come from `crypto.getRandomValues`, never `Math.random` (ST-19-4). After lock the page terminates the Worker, because linear memory cannot be wiped (AC-62). 19 names "Vite + SvelteKit"; the web app is a Svelte 5 + Vite SPA (module-layout 16), so the probe uses the web app's build as recorded by E-26/c01.

## Outcome

- `wasm-bindgen` exports for every operation of `vault_core_api.md`, chunk-sized only, with typed errors carrying the `VaultError` name and non-secret fields.
- Glue that fills `RandomSource` buffers from `crypto.getRandomValues`; the core module has no randomness import.
- Handles freed explicitly; `lock` frees and zeroizes.
- A probe in the web app's build: a module Worker encrypts and decrypts a 100 MiB file chunk by chunk under the strict CSP, with heap and Worker memory recorded in Chromium and WebKit.
- The WASM module size recorded (with and without `passphrase-policy`).

## Scope

### Included
- Exports, glue, error mapping, handle lifetime.
- The 100 MiB probe (throwaway page; the production page is E-26/n01).
- WASM tests run under Node (`wasm-pack test --node`) and headless Chromium.

### Excluded
- The vault page, its CSP and the Worker termination on lock (E-26/n01).
- The three-target vector gate (E-29/c12).

## Acceptance Criteria

**AC-1: Exports match the native API**
Given the WASM build
When the IT-19-2 round trip and the IT-19-3 tamper cases run through the exports in Node and headless Chromium
Then results equal the native results case by case, and every error arrives as a typed error with the `VaultError` name

**AC-2: Randomness from the browser only (ST-19-4 part 3)**
Given the glue and the module
When the randomness gate runs
Then the glue uses `crypto.getRandomValues`, never `Math.random`, the module imports no randomness function, and a seeded `Math.random` call fails the gate

**AC-3: Block-sized calls**
Given the exports
When a 100 MiB file is processed
Then no export accepts or returns more than one chunk plus its tag, and the Worker heap stays near one chunk above the unlocked baseline

**AC-4: 100 MiB in a module Worker under the strict CSP**
Given the web app's production build per the E-26/c01 record and the CSP of 16 §5
When the probe encrypts and decrypts a 100 MiB file in Chromium and WebKit
Then both round-trip with equal hash, the page reports 0 CSP violations, and peak heap and Worker memory are recorded (Safari on the owner Mac when available, otherwise `NOT-RUN(H-MAC)`)

**AC-5: Lock frees the handles**
Given an unlocked vault and an open decryptor in the Worker
When `lock` is called
Then later calls on those handles return a typed error, and the record states that the page must terminate the Worker

## Non-Functional Requirements

**Performance**
- Unlock at most 2 s in the browser at the S7 defaults (measured at system tier, NFT-PERF-29).

**Compatibility**
- `wasm-bindgen =0.2.130`, `wasm-pack` 0.15.x, `--target web`; no threads.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Randomness gate on real and seeded glue | Pass, then fail |
| AC-3 | Export signatures | No whole-file parameter |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Node and headless Chromium | IT-19-2, IT-19-3 through exports | Equal to native | AC-13, AC-60 |
| AC-4 | Web app build, strict CSP | 100 MiB in a module Worker | Round-trip; 0 CSP violations; memory recorded | AC-14, NFT-RES-LIM-08 |
| AC-5 | Worker with handles | Lock | Handles unusable | AC-62 |

## Constraints

- The core module stays pure; only the glue touches browser APIs.

## Runtime Completeness

- **Capability**: the WASM build of the vault core in a module Worker.
- **Production code that must exist**: the exports and glue used by the vault page.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: WebCrypto or a JavaScript library reimplementing the vault format.

## Risks & Mitigation

**Risk 1: `wasm-unsafe-eval` needed**
- *Risk*: the CSP needs `'wasm-unsafe-eval'`, which AC-62's review must accept.
- *Mitigation*: the E-26/c01 record states it per engine; the review plan (E-29/c01) includes it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-00's check allows `wasm-bindgen` only in `sync-core-wasm` (AZ-3739 AC-3); this adapter needs an allowlist change or its own adapter crate (owner, see E-29/c04) | Owner / E-00 | open | Medium |
| 19 §5 names "Vite + SvelteKit" while the web app is a Svelte 5 + Vite SPA; the probe follows 16 | E-29 / E-26 owners | open | Low |
