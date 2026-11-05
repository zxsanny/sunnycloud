# UniFFI adapter for the vault core: Kotlin and Swift bindings with block-sized calls [v1.2]

**Task**: AZ-4014_vault_uniffi_adapter
**Name**: Vault UniFFI adapter
**Description**: Expose the vault operations through UniFFI for Kotlin and Swift with objects for the vault and the file encryptor and decryptor, block-sized calls only and `VaultError` as a typed error, tested on a JVM and on the owner Mac
**Complexity**: 3 points
**Dependencies**: AZ-4008_passphrase_policy, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4012_lock_zeroize, AZ-3968_sync_uniffi_surface
**Component**: 19 vault_core [v1.2] (UniFFI target)
**Tracker**: AZ-4014
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — "UniFFI form" (this task contributes it).
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — the workspace UniFFI version (E-23/c15).
- ADR-013 (UniFFI only, the native wrapper owns the handle).

## Problem

Mobile hosts call the vault core through UniFFI, compiled into the same library as 13 (19 §5). The native wrapper must own the `UnlockedVault` handle and pass content one block at a time so memory stays bounded on 4 GB Android phones (AC-14). The bindings must come from the same sources as native and WASM so the vector file gives identical results (IT-19-1).

## Outcome

- UniFFI objects for `UnlockedVault`, `FileEncryptor`, `FileDecryptor`; functions for the other operations; `VaultError` as a UniFFI error enum with the same names.
- Kotlin and Swift bindings generated with the workspace UniFFI version, no manual edits.
- Tests on a JVM (Kotlin) and on the owner Mac (Swift; `NOT-RUN(H-MAC)` elsewhere, blocking only the release gate).

## Scope

### Included
- The `uniffi` feature of `vault-core` and its exports; binding generation; JVM and Swift smoke and round-trip tests.

### Excluded
- Assembly of `vault-core` into the Android and iOS libraries and any mobile vault screen (not planned; see Flagged concerns).
- The vector gate (E-29/c12).

## Acceptance Criteria

**AC-1: Bindings generate**
Given the `uniffi` feature on
When Kotlin and Swift bindings are generated
Then both succeed without manual edits, every operation and error of `vault_core_api.md` is present, and with the feature off no UniFFI crate is in the tree

**AC-2: Round trip on the JVM**
Given the Kotlin bindings on a JVM
When the IT-19-2 sizes are encrypted and decrypted and the IT-19-11 swap set is opened
Then results equal the native results case by case

**AC-3: Block-sized calls and handle ownership**
Given the exported surface
When its signatures are checked
Then no call takes or returns more than one chunk plus its tag, the vault handle cannot be cloned on the foreign side, and `lock` makes later calls on the handle fail with a typed error

**AC-4: Swift leg**
Given the owner Mac
When AC-2 runs with the Swift bindings
Then results equal native; without the Mac the leg is `NOT-RUN(H-MAC)`, never PASS

## Non-Functional Requirements

**Compatibility**
- One `uniffi` version in the workspace lockfile (13 risk 6).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Feature on and off dependency trees | UniFFI only with the feature |
| AC-3 | Exported signatures | Block-sized only |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | JVM | IT-19-2 and IT-19-11 through Kotlin | Equal to native | AC-60, AC-13 |
| AC-4 | Owner Mac | Same through Swift | Equal, or `NOT-RUN(H-MAC)` | RESTRICT-23 |

## Constraints

- UniFFI only; no flutter_rust_bridge (ADR-013).

## Runtime Completeness

- **Capability**: the UniFFI build of the vault core.
- **Production code that must exist**: the exported surface and generated bindings.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: a Kotlin or Swift reimplementation of any vault primitive.

## Risks & Mitigation

**Risk 1: Bindings drift from the Rust surface**
- *Risk*: generated files fall behind.
- *Mitigation*: the vector gate (E-29/c12) runs through the bindings on every change.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No planned task assembles `vault-core` into the Android and iOS libraries for v1.2 or builds a mobile vault screen, so the UniFFI surface has no production caller (AZ-3675 flags the same for clients) | Owner / E-27, E-28 | open | Medium |
| The mobile randomness source (foreign callback or OS source) is not stated (`vault_core_api.md` open point 5) | Owner / reviewer | open | Low |
