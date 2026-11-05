# Spike: load `sync-core-wasm` in a Vite-built module Worker under the strict CSP

**Task**: AZ-3930_vite_wasm_spike
**Name**: Vite and wasm-bindgen integration spike
**Description**: Prove and record how the production build packages the wasm-bindgen `--target web` output of `sync-core-wasm` and loads it, single-threaded, in a module Web Worker under the required CSP in Chromium, Firefox and WebKit
**Complexity**: 3 points
**Dependencies**: AZ-3739_sync_core_skeletons, AZ-3652_initial_structure
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3930
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c01

## Problem

The browser runs every byte-exact protocol rule through the WASM build of the sync core (ADR-001, 16 §1 boundary). How the Vite build turns the wasm-bindgen package into hashed static assets, and how a module Worker loads it with no inline script and only `script-src 'self' 'wasm-unsafe-eval'`, is unproven (16 §15 risk 1, epic risk 2). The scaffold (E-26/c06), the large-upload Worker (E-26/c10) and the S12 spike (E-26/c02) all build on this answer.

## Outcome

- A written decision in `_docs/02_document/components/16_web_app/description.md` §5 (Key dependencies) and §15 risk 1: the build integration chosen, how the Worker gets the WASM URL, and which CSP tokens each engine needs.
- Measured numbers recorded with it: WASM size (raw and compressed) and first instantiate time per engine.
- E-26/c06 and E-26/c10 can start from the record; the probe code is not merged.

## Scope

### Included
- A throwaway probe project with the pinned Svelte 5, Vite and TypeScript versions.
- One probe export (for example a BLAKE3 block hash) added only in the spike's branch of the WASM adapter crate that E-00/c07 created.
- Production build served by a static server that sends exactly the CSP of 16 §5.
- Load checks in Chromium, Firefox and WebKit (`browsers` image); Safari on the owner Mac when available.

### Excluded
- The real scaffold (E-26/c06), real WASM exports (E-23/c16), the Worker transfer protocol (E-26/c10).

## Acceptance Criteria

**AC-1: Module Worker loads the WASM in three engines**
Given the probe production build served with exactly the CSP of 16 §5
When a module Worker loads the packaged WASM and calls the probe export in Chromium, Firefox and WebKit
Then each engine returns the expected value, the page reports 0 CSP violations, and the main thread instantiates no WASM

**AC-2: Need for `'wasm-unsafe-eval'` recorded**
Given the same build
When it is served once with `'wasm-unsafe-eval'` removed from `script-src`
Then the record states per engine whether instantiation fails, which is the input to epic risk 3 and the v1.2 vault review under AC-62

**AC-3: Build output hygiene**
Given the production build
When its output is scanned
Then the WASM and Worker files have content-hashed names, `index.html` contains 0 inline scripts and 0 inline handlers, and no asset refers to a host other than the page's own origin

**AC-4: Decision recorded**
Given the measurements of AC-1 to AC-3
When the spike closes
Then the 16 description records the chosen integration, the WASM sizes and the instantiate time per engine, risk 1 is closed or restated with the remaining gap, and no probe code is on the main branch

## Non-Functional Requirements

**Compatibility**
- Single-threaded WASM only: no shared memory, no cross-origin isolation headers (16 §5, 19 §WebAssembly).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Scan of the build output for inline scripts, inline handlers, foreign hosts and unhashed WASM or Worker names | 0 findings on the probe build; a seeded inline script is found |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Probe build behind a static server with the 16 §5 CSP | Worker load and probe call in Chromium, Firefox, WebKit | Expected value; 0 CSP violations; no WASM on the main thread | IT-16-9 (CSP set), IT-16-18 (precursor) |
| AC-2 | Same build, CSP without `'wasm-unsafe-eval'` | Instantiate in each engine | Result per engine recorded | epic risk 3 |

## Constraints

- No third implementation of the protocol: the probe export comes from the Rust crate, never a JavaScript hash library.
- The spike's branch is not merged; only the record is.

## Risks & Mitigation

**Risk 1: WebKit is not Safari**
- *Risk*: Playwright WebKit can load what Safari refuses (environment.md says it is not a substitute).
- *Mitigation*: run AC-1 on the owner Mac in Safari; without the Mac, record `NOT-RUN(HW-DEVICE)` for Safari and make E-26/c10 repeat the check there.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The Safari half depends on the owner Mac (H-MAC); without it the record carries `NOT-RUN(HW-DEVICE)` for Safari | Owner (hardware), E-31/c01 inventory | open | Low |
