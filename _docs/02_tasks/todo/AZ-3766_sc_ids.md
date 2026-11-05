# sc_ids: ids, block and file hashing, commit keys, tokens, WASM build, shared vectors

**Task**: AZ-3766_sc_ids
**Name**: sc_ids ids, hashing, rollup, commit key, tokens, WASM build, shared vector file
**Description**: Byte-exact identifiers, BLAKE3 block and file hashes with the fixed rollup rule, deterministic mobile commit ids and the token rules, built natively and for WASM, with a shared known-answer vector file as a CI gate
**Complexity**: 5 points
**Dependencies**: AZ-3765_sc_clock, AZ-3761_ci_skeleton, AZ-3652_initial_structure
**Component**: shared/ids_hashing (helper 05)
**Tracker**: AZ-3766
**Epic**: AZ-3633 (E-04)
**Ref**: E-04/c01

## Document Dependencies

- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Clock` for `new_id`, produced by E-03/c01.

## Problem

"Server has hash" checks, resume diffs and idempotent commits work only if the server, the desktop and mobile cores and the web hasher compute the same block hash, file hash and commit id. A drifted rollup silently breaks verification. Tokens must be unguessable, stored only as hashes and compared in constant time (AC-12, AC-58, AC-75, AC-76).

## Outcome

- Every hash, id and token rule exists once and gives identical bytes on native and WASM targets.
- A shared vector file fails the PR gate when any target differs.
- Raw tokens never appear in logs or `Debug`.

## Scope

### Included
- `new_id` (UUIDv7 from the injected clock).
- `hash_block`, `FileHasher` (`update`, `finish_block`, `finish`) with `BlockTooLarge` above 2 MiB, and `rollup` (BLAKE3 `derive_key`, context `sunnycloud file v1`, over the ordered 32-byte block hashes; empty list for a zero-length file).
- `derive_client_key` (keyed BLAKE3 with context `sunnycloud commit v1`, first 16 bytes, UUID version and variant bits set).
- `new_token` (256-bit CSPRNG, URL-safe base64), `new_user_code` (8 characters base-20, RFC 8628 5.1), `sha256_token`, `ct_eq`, `RngUnavailable`.
- `wasm32-unknown-unknown` build of the hashing part, block-sized calls only (`getrandom` with the JS feature on wasm32).
- The shared vector file (blocks of 0, 1, 2 MiB and 2 MiB + 1 byte) and its native test and headless WASM test, both in the PR gate.

### Excluded
- Block verification on upload and blob encryption (E-10); token storage and redemption (E-15, E-16); vault key derivation (E-29).
- The Kotlin and Swift vector runs through UniFFI (E-27, E-28).
- UUIDv4 blob ids (E-10).

## Acceptance Criteria

**AC-1: Byte-exact hashing (AC-7)**
Given the shared vector file
When the native test and the headless WASM test run
Then both produce exactly the recorded `Hash32` and `FileHash` for every case

**AC-2: Bit flip detected (FT-N-01)**
Given a block and the same block with one bit flipped
When both are hashed and rolled up
Then the block hashes differ and the file hashes differ

**AC-3: Stable mobile commit id (AC-46)**
Given one device id, one asset id and one file hash
When `derive_client_key` runs twice, in two processes
Then both results are equal, and the UUID version and variant bits are set (FT-P-32 unit part)

**AC-4: Token strength (AC-12, AC-58)**
Given 1,000,000 calls of `new_token`
When the results are decoded and compared
Then each carries 256 random bits and there are 0 collisions

**AC-5: Raw tokens never printed (NFT-SEC-16)**
Given a `RawToken` with a known value
When it is formatted with `Debug` and `Display`
Then the output is a placeholder and contains no part of the value

**AC-6: Oversized block rejected**
Given a 2 MiB + 1 byte buffer fed as one block
When `FileHasher::update` runs
Then it returns `BlockTooLarge`

**AC-7: Zero-length file**
Given an empty file
When it is hashed
Then the block list is empty and the file hash equals `rollup` of an empty list

**AC-8: Constant-time compare**
Given two equal and two unequal byte strings, and two of different lengths
When `ct_eq` runs
Then it returns true, false and false, and the comparison is done by a vetted constant-time primitive, not by an early-exit loop (review item)

**AC-9: Deterministic ids in tests**
Given a `ManualClock`
When `new_id` is called before and after a tick
Then the second id sorts after the first

## Non-Functional Requirements

**Performance**
- Hashing throughput on the reference server fits the AC-2 budget with BLAKE3 1.8.7; no per-block allocation beyond the output.

**Compatibility**
- Pins: `blake3 1.8.7`, `sha2`, `uuid` (v7), `getrandom` (JS feature on wasm32); no `wasm-bindgen` in this crate.

**Security**
- Only `sha256_token` output is stored by callers; `user_code` is not a bearer secret.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Known-answer vectors natively | Recorded values |
| AC-2 | Bit-flip pair | Different hashes |
| AC-3 | `derive_client_key` repeat and bits | Equal; bits set |
| AC-4 | 1,000,000 token draws | 0 collisions |
| AC-5 | `Debug`/`Display` of `RawToken` | Placeholder |
| AC-6 | 2 MiB + 1 byte block | `BlockTooLarge` |
| AC-7 | Empty file | Empty list rollup |
| AC-8 | `ct_eq` cases and length mismatch | Correct results |
| AC-9 | `new_id` with `ManualClock` | Ordered |
| — | `new_user_code` length and alphabet | 8 characters from the base-20 set |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Upload of conformance blocks (through E-14) | FT-P-05 | Server and client agree on block hashes | Performance |
| AC-2 | Bit flip in transit and at rest (through E-14, E-10) | FT-N-01, FT-N-02 | Detected; never delivered as a good file | — |
| AC-3 | Reinstall of the Android app (through E-27) | FT-P-32 | Same commit ids after reinstall | — |
| AC-4 | Share tokens (through E-16) | FT-P-38, NFT-SEC-16 | Random, stored hashed, absent from logs | Security |

## Constraints

- Block size fixed at 2 MiB (RESTRICT-11); the context strings are frozen for v1.
- Desktop and web use random UUIDv7 commit ids; only mobile uses `derive_client_key`.

## Risks & Mitigation

**Risk 1: Rollup drift between implementations**
- *Risk*: a client computes a different file hash.
- *Mitigation*: the vector file is a gate for every consumer (native and WASM here, UniFFI in E-27).

**Risk 2: RNG unavailable on a target**
- *Risk*: a target has no CSPRNG.
- *Mitigation*: `RngUnavailable` is typed; callers fail the operation.

## Runtime Completeness

- Named capability: BLAKE3 hashing (native and WASM) and the CSPRNG token source.
- Production code that must exist: the real BLAKE3 1.8.7 rollup and keyed derivation on both targets, OS randomness and `crypto.getRandomValues` in the browser.
- Allowed external stubs: none.
- Unacceptable substitutes: a non-BLAKE3 hash, a seeded or deterministic RNG outside tests, a WASM build that skips the vector test.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| data_model §1 says tokens carry a type prefix; the prefix strings and the exact `TokenKind` list are not specified. | E-04 owner with E-15, E-16 | open | Medium |
| Helper 05 names 19 vault_core as a user, but 19 has no sunnycloud dependency (module-layout open point 5). | E-29 owner | resolved | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
