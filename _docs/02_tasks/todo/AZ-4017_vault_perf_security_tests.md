# Vault performance and security tests PT-19-1..2, ST-19-1..4 [v1.2]

**Task**: AZ-4017_vault_perf_security_tests
**Name**: Vault performance and security suites
**Description**: Measure unlock time and 100 MiB memory, and run the security suites: no secrets in any text output, zeroized memory after lock and drop, constant-time comparisons, and unique nonces from the injected source
**Complexity**: 3 points
**Dependencies**: AZ-4005_argon2_s7_benchmark_spike, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4012_lock_zeroize, AZ-4013_vault_wasm_adapter
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4017
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — invariants under test.
- `components/19_vault_core/tests.md` PT-19-1, PT-19-2, ST-19-1 to ST-19-4; record of E-29/c02 (default parameters).

## Problem

AC-14 bounds unlock time (1 s native, 2 s browser) and memory (block-sized processing of 100 MiB). AC-62 requires no key in memory after lock. Key material must never reach logs or crash reports (RESTRICT-23), comparisons of secrets must be constant-time, and every nonce must be unique under one key. These are suite-level checks over the whole crate.

## Outcome

- PT-19-1: native unlock p95 at the defaults on the REF-SRV class CPU (workstation runs are `NOT-RUN(HW-REF)`); browser numbers come from NFT-PERF-29 at system tier.
- PT-19-2: peak allocation above the unlocked baseline while encrypting and decrypting 100 MiB in 4 MiB chunks.
- ST-19-1 to ST-19-4 as CI suites.

## Scope

### Included
- PT-19-1, PT-19-2, ST-19-1, ST-19-2 (native), ST-19-3, ST-19-4.

### Excluded
- The WASM heap scan after Worker termination (system tier, NFT-SEC-42 step 3, E-31 with E-26/n01).
- Fuzzing (E-29/c13).

## Acceptance Criteria

**AC-1: Unlock time (PT-19-1, AC-14)**
Given the default parameters from E-29/c02 on the REF-SRV class CPU
When 20 sequential `unlock` calls run on one thread
Then p95 is at most 1 s and peak memory at most Argon2 memory + 8 MiB; on a workstation the result is `NOT-RUN(HW-REF)`, never a substitute

**AC-2: Memory over 100 MiB (PT-19-2, AC-14)**
Given a 100 MiB file in 4 MiB chunks and a counting allocator
When it is encrypted and then decrypted through the incremental API
Then peak allocation above the unlocked baseline is at most 12 MiB (failure above 16 MiB)

**AC-3: No secrets in text (ST-19-1)**
Given 1,000 failing calls from the IT-19-3 to IT-19-11 cases
When every `Display`, `Debug` and panic message is captured
Then the passphrase, the recovery key and any 8-byte window of the vault key and file keys occur 0 times

**AC-4: Zeroized after lock and drop (ST-19-2 native, AC-62)**
Given an unlocked vault that decrypted a file
When `lock` is called, and in a second run the values are dropped without `lock`, and the writable mappings of the test process are scanned
Then the vault key, the KEK and the file keys occur 0 times

**AC-5: Constant-time comparisons (ST-19-3)**
Given the CI source gate
When it runs over the crate and over a seeded `==` on a commitment
Then findings are 0 on the crate and 1 on the seed

**AC-6: Randomness and nonces (ST-19-4)**
Given 10,000 wraps and 1,000 file encryptions with the OS source, and a failing `RandomSource`
When the 24-byte nonces and 19-byte prefixes are collected, and the failing source is used
Then duplicates are 0, the failing source gives `Rng` with 0 bytes of output, the WASM glue never calls `Math.random`, and the crate calls no system randomness outside the native OS-source feature

## Non-Functional Requirements

**Performance**
- Targets as in the criteria; the 250 MB browser totals are NFT-RES-LIM-08 (system tier).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Counting allocator over 100 MiB | At most 12 MiB above baseline |
| AC-3 | Text capture over 1,000 failing calls | 0 matches |
| AC-5 | Source gate on crate and seed | 0 and 1 findings |
| AC-6 | Nonce collection; failing source | 0 duplicates; `Rng` with no output |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | REF-SRV class CPU | 20 unlocks | p95 at most 1 s | PT-19-1, NFT-PERF-29 |
| AC-4 | Native process | Lock and drop, memory scan | 0 key bytes | ST-19-2, NFT-SEC-42 |

## Constraints

- No hardware substitute: a workstation number never stands in for REF-SRV.

## Risks & Mitigation

**Risk 1: Memory scan misses copies in registers or swap**
- *Risk*: the scan sees only writable mappings.
- *Mitigation*: the stated limit; the external review covers it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The REF-SRV class machine is owner hardware; PT-19-1 may stay `NOT-RUN(HW-REF)` | Owner | open | Low |
