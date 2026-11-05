# Known-answer vector file from an independent oracle and the three-target CI gate [v1.2]

**Task**: AZ-4015_vault_vectors_ci_gate
**Name**: Vault vectors and CI gate
**Description**: Generate the vault known-answer vector file once with an oracle that shares no code with the crate, and gate CI on byte-identical results on native, WASM and Kotlin (Swift at the release gate)
**Complexity**: 3 points
**Dependencies**: AZ-4009_vault_header_format, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4013_vault_wasm_adapter, AZ-4014_vault_uniffi_adapter
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4015
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — the only input of the oracle.
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — operations under test.
- `components/19_vault_core/tests.md` IT-19-1; Test Data Management (vector file, oracle).

## Problem

The same code compiled three ways must give identical bytes (RESTRICT-23), and the format must be rebuildable from its written description, which is what the reviewer and any future client rely on. A vector file produced by the crate itself would only prove the crate agrees with itself. IT-19-1 requires an oracle in another language from other libraries.

## Outcome

- An oracle (Python: `argon2-cffi`, PyNaCl XChaCha20-Poly1305, `blake3`, and a short STREAM BE32 routine written from `vault_format.md`) that writes `vectors/vault-v1.json`, committed as a fixture and regenerated only when `format_version` changes.
- A deterministic `RandomSource` that replays the bytes listed in the file.
- Vectors: Argon2id (m = 64 MiB, t = 3, p = 1; 2 vectors) and the RFC 9106 vector through the internal KDF behind the bounds check; both vault-key wraps; the key commitment; a file-key wrap; content for an empty file, one chunk, 150,000 bytes at 64 KiB (65,536 + 65,536 + 18,928) and 3 chunks at 4 MiB (SHA-256 of the ciphertext); full headers at generations 1 and 2.
- A CI gate on native, WASM (Node and headless Chromium) and Kotlin on a JVM; Swift on the owner Mac before the release.

## Scope

### Included
- The oracle tool, the vector file, the replaying `RandomSource`, the gate jobs.
- Test IT-19-1.

### Excluded
- Fuzz corpora (E-29/c13); performance and security suites (E-29/c14).

## Acceptance Criteria

**AC-1: Independent oracle**
Given the oracle and the crate
When their dependency lists are compared
Then the oracle imports no part of the crate and uses only the listed libraries and its own STREAM routine written from `vault_format.md`

**AC-2: Byte-identical on every target (IT-19-1)**
Given `vectors/vault-v1.json`
When the gate runs on native, WASM in Node, WASM in headless Chromium and Kotlin on a JVM
Then every output byte equals the file, decrypting each vector ciphertext returns its plaintext, and each target finishes within 60 s

**AC-3: Gate fails on drift**
Given a seeded one-byte change in one vector, and separately a seeded change in the crate's header serialisation
When the gate runs
Then it fails in both cases and names the vector

**AC-4: Swift at the release gate**
Given the owner Mac
When the gate runs through the Swift bindings
Then results equal the file; without the Mac the leg is `NOT-RUN(H-MAC)`, and the release checklist blocks on it

## Non-Functional Requirements

**Reliability**
- The vector file changes only through a reviewed commit that also bumps `format_version`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Gate on a seeded vector and a seeded serialisation change | Fails; vector named |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | CI runner | Native, WASM Node, WASM Chromium, Kotlin | Byte-identical; within 60 s each | IT-19-1, RESTRICT-23 |
| AC-4 | Owner Mac | Swift leg | Identical or `NOT-RUN(H-MAC)` | AC-62 |

## Constraints

- The oracle is not shipped and not a dependency of any build.

## Runtime Completeness

- **Capability**: an independent reference implementation of the vault format.
- **Production code that must exist**: the oracle and the committed vector file.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: vectors produced by the crate itself; vectors copied from library docs without the oracle run.

## Risks & Mitigation

**Risk 1: The oracle and the crate share a misreading of the spec**
- *Risk*: both follow the same wrong reading.
- *Mitigation*: the external review reads the vectors and the format (E-29/c15).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The oracle tool's location is not in module-layout's ownership tables | Owner | open | Low |
