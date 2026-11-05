# Vault header format, serialisation and bounds validation before any Argon2 work [v1.2]

**Task**: AZ-4009_vault_header_format
**Name**: Vault header format and bounds
**Description**: Serialise and parse the vault header of `vault_format.md`, and reject unknown versions, malformed lengths and out-of-bounds KDF parameters with typed errors before any key derivation, so a hostile server cannot make a client spend time or memory
**Complexity**: 3 points
**Dependencies**: AZ-4004_vault_review_plan_spike, AZ-4005_argon2_s7_benchmark_spike, AZ-4007_vault_core_skeleton
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4009
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — produced by this task (vault header part).
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — `HeaderMalformed`, `UnknownVersion`, `ParamsOutOfBounds{field}` (E-29/c04).
- Records of E-29/c01 (format freeze) and E-29/c02 (WASM and mobile caps).

## Problem

The header comes from an untrusted server, and its KDF parameters decide how much time and memory Argon2id takes. A header with `t = u32::MAX` or `m` of 64 GiB must fail at once, with no derivation (AC-61, NFT-SEC-41). Bounds: `t` 3 to 10, `m` from 64 MiB to the platform cap (512 MiB native; WASM and mobile caps from S7), `p = 1`, salt 16 bytes, known `format_version` only; chunk size 64 KiB to 4 MiB where the header carries it.

## Outcome

- Header serialisation and parsing per `vault_format.md`; every structural problem is `HeaderMalformed`, an unknown version `UnknownVersion`, a bound violation `ParamsOutOfBounds{field}` naming the field.
- Bounds checked before any Argon2 work, with per-platform memory caps.
- The parser is the entry point of the header fuzz target (E-29/c13).

## Scope

### Included
- Field layout, lengths, version check, bounds table, platform caps (native 512 MiB; WASM and mobile from E-29/c02).
- A test-only derivation counter and a counting allocator to prove "no Argon2 work".
- Test IT-19-5 and the structural part of IT-19-4.

### Excluded
- Key derivation, unwrapping and authentication (E-29/c07); file headers (E-29/c08).

## Acceptance Criteria

**AC-1: Rejected bounds cost nothing (IT-19-5, AC-61, NFT-SEC-41)**
Given structurally valid headers with t in {0, 1, 2, 11, 255, u32::MAX}, m in {63 MiB, 513 MiB, 64 GiB, u32::MAX KiB}, p in {0, 2, 4}, salt of 15 and 17 bytes, and format version 0, current + 1 and 0xFF
When `unlock` and `create_vault` are called with them
Then each returns `ParamsOutOfBounds{field}` naming the field (version cases: `UnknownVersion`; salt length: `HeaderMalformed` or `ParamsOutOfBounds`), with the derivation counter at 0, peak allocation below 8 MiB and elapsed time below 50 ms

**AC-2: Accepted bounds run exactly one derivation (IT-19-5)**
Given headers with t 3 and 10, m 64 MiB and 512 MiB (native), p 1
When `unlock` runs on each
Then each runs exactly one derivation before any other result

**AC-3: Platform caps (E-29/c02)**
Given the WASM and mobile caps recorded by E-29/c02
When headers with m at the cap and at the cap plus 1 KiB are opened in the WASM and UniFFI builds
Then the cap is accepted and the cap plus 1 KiB is `ParamsOutOfBounds{field: m}` with no derivation

**AC-4: Structural errors (IT-19-4 structural part)**
Given headers truncated by 1 byte, extended by 1 byte, and empty
When they are parsed
Then each returns `HeaderMalformed`, and no error text contains a field value or secret

**AC-5: Chunk-size bound**
Given a chunk size of 64 KiB - 1, 64 KiB, 4 MiB, 4 MiB + 1, 0 and u32::MAX wherever the format carries it
When it is parsed
Then 64 KiB and 4 MiB are accepted and the others are `ParamsOutOfBounds{field: chunk_size}`

## Non-Functional Requirements

**Performance**
- Every rejection within 50 ms and under 8 MiB (IT-19-5).

**Reliability**
- The parser never panics on any input (fuzzed in E-29/c13).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Rejected bounds table with counter and allocator | Typed error; 0 derivations; < 8 MiB; < 50 ms |
| AC-2 | Accepted bounds | One derivation each |
| AC-4 | Truncated, extended, empty headers | `HeaderMalformed` |
| AC-5 | Chunk-size table | Only the bounds accepted |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | WASM build in Node | Rejected bounds table | Same results as native | FT-N-16, NFT-SEC-41 |
| AC-3 | WASM and UniFFI builds | Cap and cap + 1 KiB | Accepted; rejected | AC-61 |

## Constraints

- The format is the one frozen by E-29/c01; any change bumps `format_version`.

## Risks & Mitigation

**Risk 1: Bound checked after a partial derivation**
- *Risk*: a refactor moves the check after allocation.
- *Mitigation*: AC-1's derivation counter and allocator run in CI.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether the vault header carries a chunk size (IT-19-5, FT-N-16) or only the file header does (19 §3) is not consistent (`vault_format.md` open point 3) | Owner / reviewer (E-29/c01) | open | Medium |

## Contract

This task produces/implements the vault-header part of the contract at `_docs/02_document/contracts/19_vault_core/vault_format.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
