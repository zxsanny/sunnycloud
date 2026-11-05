# cargo-fuzz targets for the header parser and chunk decryptor, and the 10,000-mutation test [v1.2]

**Task**: AZ-4016_vault_fuzz_targets
**Name**: Vault fuzzing
**Description**: Fuzz the vault header parser and the chunk decryptor with seeded corpora, run the 10,000-mutation header test, and replay the same corpus in V8 through the WASM build, so hostile server data can only produce typed errors
**Complexity**: 3 points
**Dependencies**: AZ-4009_vault_header_format, AZ-4011_file_encryption_stream, AZ-4013_vault_wasm_adapter
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4016
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — header and chunk layout.
- `components/19_vault_core/tests.md` ST-19-5, IT-19-4 (b); Test Data Management (DS-19 `vault-fuzz`, header generator).

## Problem

Headers and ciphertext come from an untrusted server (AC-61, AC-13). A parser or decryptor that panics, runs out of memory or loops on crafted input lets the server crash or freeze the client. AC-61 requires 0 of 10,000 fuzzed header mutations accepted. The browser build must behave the same on the same inputs.

## Outcome

- Two cargo-fuzz targets: header parsing and unlock, and chunk decryption, seeded from DS-19 and the IT-19-3 cases.
- DS-19: 10,000 seeded mutations (bit flips at every offset, truncations, extensions) of a header written by the header generator; the 10,000-mutation test with 0 accepted.
- CI smoke 10 minutes per target, nightly 2 hours; the same corpus replayed in V8 through the WASM build.

## Scope

### Included
- Targets, corpora, the DS-19 generator, the mutation test, CI and nightly jobs, the V8 replay.
- Tests ST-19-5 and IT-19-4 (b).

### Excluded
- The parser and decryptor themselves (E-29/c06, c08).

## Acceptance Criteria

**AC-1: 10,000 mutations rejected (IT-19-4 b, AC-61, FT-N-16, NFT-SEC-41 step 1)**
Given DS-19
When each mutated header is unlocked
Then 0 are accepted, each fails with a typed error, and the run finishes within its 600 s budget using the test-only KDF hook outside the KDF fields

**AC-2: No crash, no exhaustion (ST-19-5)**
Given both targets seeded from DS-19 and IT-19-3
When the CI smoke runs 10 minutes per target and the nightly run 2 hours
Then panics, out-of-memory kills and inputs running over 1 s are 0; any finding is saved as a reproducer and fails the job

**AC-3: Same results in V8**
Given the corpus and the WASM build
When the corpus is replayed in Node
Then every input gives the same result class (typed error name or success) as native, and none crashes

## Non-Functional Requirements

**Reliability**
- Fuzz findings are kept as regression inputs in the corpus.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | 10,000-mutation test | 0 accepted |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | CI and nightly runners | Fuzz both targets | 0 crashes, OOMs, slow inputs | ST-19-5, AC-61 |
| AC-3 | Node with the WASM build | Corpus replay | Same result classes | RESTRICT-23 |

## Constraints

- Fuzzing runs on CI runners; nightly workflows are added by this epic, not E-00's PR gate.

## Risks & Mitigation

**Risk 1: Fuzz job slows the PR gate**
- *Risk*: 20 minutes added to every pull request.
- *Mitigation*: the smoke runs only when `vault-core` changes; the long run is nightly.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The nightly workflow is outside E-00's PR gate (AZ-3761 leaves nightly workflows to later epics); its runner budget is not stated | Owner / E-00 | open | Low |
