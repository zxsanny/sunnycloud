# `vault-core` crate skeleton: `no_std + alloc`, pins, `RandomSource`, `VaultError`, zeroize, three-target CI [v1.2]

**Task**: AZ-4007_vault_core_skeleton
**Name**: vault-core skeleton
**Description**: Set up `vault-core` as a pure library with no sunnycloud dependency and no global state, exact crypto pins, the injected `RandomSource`, the complete `VaultError` set, the format and bound constants, and a CI matrix that builds and tests it natively, as WASM and through UniFFI
**Complexity**: 3 points
**Dependencies**: AZ-4004_vault_review_plan_spike, AZ-3652_initial_structure
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4007
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — produced by this task (types, errors, constants, build targets).
- Record of E-29/c01 (format freeze, fallback decision that may change the pins).

## Problem

The same vault code must run in a browser Worker, on Android and iOS and in native tests with identical outputs (RESTRICT-23), so it must stay small and auditable: no sunnycloud crate, no `contracts`, no I/O, no global state (19 §1, module-layout open point 5). `getrandom` has no `wasm32-unknown-unknown` backend by default, so randomness is injected. Every later E-29 task needs the crate, its error set and a CI that runs all three targets.

## Outcome

- `vault-core` builds `no_std + alloc` with the pins of 19 §1 (`chacha20poly1305 =0.11.0` default features off with `alloc`, `aead-stream =0.6.0`, `argon2 =0.6.0` without `parallel`, `zeroize`, `blake3` 1.8.7, `zxcvbn` behind feature `passphrase-policy`, `subtle`), or the fallback pins decided by E-29/c01; `Cargo.lock` committed.
- `RandomSource` trait; an OS-backed source behind a native-only feature.
- The complete `VaultError` set of `vault_core_api.md`; constants: `format_version` 1, KDF bounds (m from 64 MiB to the platform cap, t 3 to 10, p = 1), chunk bounds (64 KiB to 4 MiB), default `max_file_bytes` 104,857,600.
- A CI matrix: native `cargo test -v`; `wasm32-unknown-unknown` build with no `getrandom` and `wasm-pack test --node`; UniFFI Kotlin bindings on a JVM; Swift on the owner Mac (`NOT-RUN(H-MAC)` elsewhere, blocking only the release gate).

## Scope

### Included
- Crate setup, features (`passphrase-policy`, native OS randomness, `uniffi`, the WASM adapter feature), pins, the error enum and constants, `RandomSource`.
- Crate-graph checks: no sunnycloud crate and no `contracts` in the dependency tree; no `getrandom` in the WASM tree.
- `cargo deny` licence allowlist passing for the iOS build (no GPL crate).
- The CI matrix jobs (v1.2 only; not part of v1.0 or v1.1 release builds).

### Excluded
- Operations (E-29/c05 to c09); the WASM and UniFFI exports themselves (E-29/c10, c11); the vector gate (E-29/c12).

## Acceptance Criteria

**AC-1: Pure crate**
Given the crate on the pinned toolchain
When the crate-graph check runs
Then the dependency tree holds no sunnycloud crate, no `contracts`, no I/O or network crate, and a seeded dependency on a sunnycloud crate fails the check

**AC-2: Builds on three targets**
Given a clean checkout
When the native build and tests, the `wasm32-unknown-unknown` build with `wasm-pack test --node`, and the UniFFI Kotlin binding generation with a JVM smoke test run
Then all pass with verbose output, and the Swift leg reports `NOT-RUN(H-MAC)` when no Mac is attached, never PASS

**AC-3: Randomness only injected**
Given the WASM dependency tree and the crate sources
When the randomness check runs
Then `getrandom` is absent from the WASM tree, and no code outside the native OS-source feature reads system randomness directly

**AC-4: Error set and constants**
Given the crate's public API
When it is compared with `vault_core_api.md`
Then every listed `VaultError` variant and constant exists with the stated values, and no variant carries a secret-typed field

**AC-5: Pins and licences**
Given `Cargo.lock`
When the pin and licence checks run
Then each crypto crate is at its exact pin and the `cargo deny` allowlist passes for the iOS build

## Non-Functional Requirements

**Compatibility**
- `wasm-bindgen =0.2.130`, `wasm-pack` 0.15.x, UniFFI 0.32.x from the workspace lockfile.

**Reliability**
- No global state; every function is pure apart from allocation and zeroization.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Crate-graph check on real and seeded trees | Pass, then fail |
| AC-3 | Randomness check on real and seeded sources | Pass, then fail |
| AC-4 | API comparison with the contract | All variants and constants present |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Clean checkout, CI runner | Three-target matrix | Native, WASM, Kotlin pass; Swift not run without Mac | RESTRICT-23 |
| AC-5 | Lockfile | Pin and licence checks | Exact pins; allowlist passes | ADR-011 |

## Constraints

- Nothing of this crate is linked into v1.0 or v1.1 release builds (ADR-016, FT-P-73).

## Runtime Completeness

- **Capability**: the three build targets of one crypto core (native, WASM, UniFFI).
- **Production code that must exist**: the crate and its target builds in CI.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: a JavaScript or Kotlin reimplementation of any vault primitive.

## Risks & Mitigation

**Risk 1: zxcvbn inflates the WASM size**
- *Risk*: the dictionary makes the module large.
- *Mitigation*: feature `passphrase-policy`; the size is measured in E-29/c05.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-00's dependency-split check allows `wasm-bindgen` only in `sync-core-wasm` (AZ-3739 AC-3, module-layout `sync-core-wasm` entry), while 19 §5 builds `vault-core` with `wasm-bindgen`; the check's allowlist needs an E-00 change or a separate vault WASM adapter crate (owner decision) | Owner / E-00, E-23 | open | Medium |
| How vault crates are kept out of v1.0 and v1.1 release builds is module-layout open point 6 | Owner / E-22, E-27, E-29, E-30 | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/19_vault_core/vault_core_api.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
