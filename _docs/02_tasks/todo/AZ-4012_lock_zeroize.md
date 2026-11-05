# `lock`, move-only `UnlockedVault`, no secrets in `Debug`, constant-time comparisons [v1.2]

**Task**: AZ-4012_lock_zeroize
**Name**: Vault lock and secret handling
**Description**: Make every key-holding type move-only and not cloneable, print no secret in `Debug` or errors, zeroize all key material on `lock` and on drop, and compare commitments, tags and checksums in constant time
**Complexity**: 3 points
**Dependencies**: AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4012
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — `lock`, `UnlockedVault` rules (this task contributes them).
- `components/19_vault_core/description.md` §2, §7; `acceptance_criteria.md` AC-62.

## Problem

After the user locks the vault, no key may remain in memory (AC-62). Key material must not leak through copies, `Debug` output, error text or panics (ST-19-1), and the commitment and checksum checks must not leak timing (ST-19-3). WASM linear memory cannot be guaranteed wiped, so the page must terminate the Worker on lock; the crate must state that as a caller duty.

## Outcome

- `UnlockedVault`, `FileEncryptor`, `FileDecryptor` and all key buffers: move-only, not `Clone`, `Debug` prints no key bytes, `Drop` zeroizes.
- `lock(vault)` consumes the vault and zeroizes the vault key and any derived key it holds; using a locked vault does not compile.
- All secret comparisons through a constant-time primitive.
- The WASM caller duty (terminate the Worker after `lock`) documented in `vault_core_api.md`.

## Scope

### Included
- Type rules, zeroization on lock and drop, constant-time comparisons.
- Unit-level proofs of each rule.

### Excluded
- The suite-level security tests ST-19-1 to ST-19-4 (E-29/c14); the Worker termination on the page (E-26/n01).

## Acceptance Criteria

**AC-1: Move-only and not cloneable (ST-19-1 part 1)**
Given compile-fail tests
When code tries to clone an `UnlockedVault`, a `FileEncryptor` or a `FileDecryptor`, or to use a vault after `lock`
Then none of it compiles

**AC-2: No key bytes in `Debug` or errors**
Given an unlocked vault with known key bytes
When `Debug` of every key-holding type and `Display` and `Debug` of every `VaultError` produced in this crate's unit tests are captured
Then no 8-byte window of any key, the passphrase or the recovery key appears

**AC-3: Zeroize on lock and drop**
Given a vault and an open decryptor whose key buffers are observed through a test-only hook
When `lock` is called, and in a second case the values are dropped without `lock`
Then every observed buffer is zero afterwards in both cases

**AC-4: Constant-time comparisons**
Given the commitment, tag and checksum checks
When the crate is inspected by the comparison rule (disallowed equality on secret byte arrays)
Then every such comparison uses the constant-time primitive, and a seeded `==` on a commitment is reported

## Non-Functional Requirements

**Reliability**
- No panic path prints a secret; panics in key-holding code are avoided by typed errors.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Compile-fail tests | Do not compile |
| AC-2 | `Debug` and error text with known keys | 0 matches |
| AC-3 | Lock and drop with observed buffers | All zero |
| AC-4 | Comparison rule on the crate and a seeded `==` | 0 findings; 1 on the seed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-3 | Native process | Lock, then memory scan (full suite in E-29/c14) | 0 key bytes | ST-19-2, AC-62 |

## Constraints

- `zeroize` for all key buffers; no key in a type that can be copied implicitly.

## Risks & Mitigation

**Risk 1: Compiler copies keys**
- *Risk*: moves leave stale copies on the stack.
- *Mitigation*: keys live in heap buffers that are zeroized; the process scan of E-29/c14 checks the result.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| WASM linear memory cannot be wiped; AC-62 relies on the page terminating the Worker (E-26/n01) | E-26 / owner | accepted-risk | Medium |
