# `create_vault`, `unlock`, `unlock_with_recovery`, `rewrap_header`: KDF, double wrap, key commitment, generation pin [v1.2]

**Task**: AZ-4010_vault_create_unlock
**Name**: Vault create, unlock and rewrap
**Description**: Create a vault with a random vault key wrapped under an Argon2id key and under a one-time recovery key, authenticate the whole header, check the key commitment after unwrap, refuse rolled-back headers, and change the passphrase with a generation increment
**Complexity**: 5 points
**Dependencies**: AZ-4007_vault_core_skeleton, AZ-4008_passphrase_policy, AZ-4009_vault_header_format
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4010
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — `create_vault`, `unlock`, `unlock_with_recovery`, `rewrap_header`, `UnlockedVault` (this task contributes them).
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — header fields, AAD, commitment (E-29/c06).

## Problem

A stolen database or blob volume must yield no plaintext (AC-13). The vault key is random and wrapped twice: under a KEK from Argon2id over the passphrase and under a key from a full-entropy recovery key (RESTRICT-22). XChaCha20-Poly1305 alone is not key-committing, so a server could craft a header that opens to a different key; a key-commitment value prevents that (IT-19-6). A server could also serve an old header (for example one with a revoked passphrase); clients pin the last generation they saw (IT-19-8). Every header byte must be authenticated (AC-61).

## Outcome

- `create_vault`: passphrase checked (E-29/c05), bounds checked (E-29/c06), 16-byte salt and 256-bit vault key and recovery key from `RandomSource`, both wraps with random 192-bit nonces and the whole serialised header (all fields except the wrapped blobs, generation included) as AAD, commitment = keyed BLAKE3 under the vault key of `"sunnycloud vault commit v1" || vault_id`; generation 1; the recovery key returned once.
- `unlock`: bounds first, one derivation, unwrap, constant-time commitment check, generation compared with `pinned_generation`.
- `unlock_with_recovery`: no Argon2; typos caught by the checksum (`RecoveryKeyInvalid`).
- `rewrap_header`: new salt and KEK, same vault key, generation + 1, never wrapping past `u64::MAX`.

## Scope

### Included
- The four operations, the recovery-key display form (grouped base32 with checksum) and its input parsing.
- Tests IT-19-6, IT-19-8, IT-19-10 and the authentication part of IT-19-4.

### Excluded
- File keys and content (E-29/c08); `lock` and handle rules (E-29/c09); the vector file (E-29/c12).

## Acceptance Criteria

**AC-1: Every header byte authenticated (IT-19-4, AC-61)**
Given a valid generation-1 header
When each byte outside the bound fields is replaced by its xor 0x01, xor 0xFF and 0x00, and the two wrapped blobs are swapped
Then `unlock` and `unlock_with_recovery` accept 0 of them, each failing with `HeaderAuthFailed`, `WrongPassphraseOrTampered` or `KeyCommitmentMismatch`, and no error text carries a field value or secret

**AC-2: Key commitment (IT-19-6)**
Given oracle-built headers whose wrap authenticates under the real KEK but unwraps to K2 while the commitment belongs to K1, whose recovery wrap unwraps to a different key than the passphrase wrap, and a genuine header of vault V1 relabelled with V2's `vault_id` and re-authenticated
When they are unlocked by passphrase and by recovery key
Then all return `KeyCommitmentMismatch` with no `UnlockedVault`, and the genuine header unlocks

**AC-3: Generation pin (IT-19-8)**
Given headers at generations 1, 2 and 3 from `create_vault` and two `rewrap_header` calls
When each is unlocked with `pinned_generation` None, 1, 2 and 3
Then generation g with pin p is accepted when g >= p and returns `HeaderRolledBack{seen: g, pinned: p}` when g < p, and None accepts every generation

**AC-4: Generation field authenticated and bounded (IT-19-8)**
Given a generation-1 header edited to 3, a generation-3 header edited to 1, and a header at `u64::MAX`
When they are unlocked or rewrapped
Then both edits fail authentication, and `rewrap_header` at `u64::MAX` returns an error and never produces generation 0

**AC-5: Rewrap keeps the recovery key (IT-19-8, IT-19-10)**
Given a vault rewrapped to a new passphrase
When it is unlocked with the old passphrase, the new passphrase and the recovery key
Then the old fails, the new works, and the recovery key unlocks every generation

**AC-6: Recovery key (IT-19-10, RESTRICT-22)**
Given the recovery key from `create_vault`
When it is used as shown, with 1,000 single-character substitutions, 1,000 adjacent transpositions, a wrong group length and lower-case input
Then the genuine key unlocks, substitutions and transpositions return `RecoveryKeyInvalid` 2,000 of 2,000 times, lower-case input follows the one documented rule, and the key's bytes appear nowhere outside the one returned value (header, errors, `Debug`)

**AC-7: Weaker parameters from the server (NFT-SEC-41 step 5)**
Given a vault created at m = 128 MiB and a server that returns a header with m = 64 MiB
When the client unlocks it
Then it is refused (authentication fails) and there is no fallback path

## Non-Functional Requirements

**Performance**
- One Argon2id derivation per `unlock` and per `create_vault`; unlock within 1 s native at the defaults (measured in E-29/c14).

**Reliability**
- Every random value comes from `RandomSource`; a failing source returns `Rng` with no output.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Byte mutations through the test-only KDF hook (KEK derived once) | 0 accepted |
| AC-2 | Oracle-built commitment cases | `KeyCommitmentMismatch`; control opens |
| AC-3 | Generation and pin table | Accept or `HeaderRolledBack` as stated |
| AC-4 | Edited generation; `u64::MAX` | Auth failure; error without wrap |
| AC-5 | Old, new passphrase and recovery after rewrap | Fail, work, work |
| AC-6 | Recovery-key mutations and byte search | 2,000 of 2,000 rejected; 0 occurrences |
| AC-7 | Weaker-parameter header | Refused |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | WASM build in Node | Mutation set | Same results as native | FT-N-16, NFT-SEC-41 |
| AC-3 | WASM and Kotlin builds | Pin table | Same results | AC-61 |

## Constraints

- Constant-time comparison for the commitment and the checksum (ST-19-3).
- No secret in `Debug`, error text or panics (ST-19-1).

## Runtime Completeness

- **Capability**: Argon2id v0x13 (`argon2 =0.6.0`), XChaCha20-Poly1305 (`chacha20poly1305 =0.11.0`), keyed BLAKE3.
- **Production code that must exist**: the real KDF, wraps and commitment in every build.
- **Allowed stubs**: the test-only KDF hook that reuses one derived KEK inside mutation tests.
- **Unacceptable substitutes**: a reduced or skipped KDF outside tests, a non-AEAD wrap, a commitment compared with `==`.

## Risks & Mitigation

**Risk 1: Wrong-passphrase and tamper look the same**
- *Risk*: users cannot tell a typo from an attack.
- *Mitigation*: the errors distinguish only what the construction allows (19 §7); how the page words them is E-26/n01's.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `VaultError` variant for generation overflow is not named (`vault_core_api.md` open point 1) | Owner / reviewer | open | Low |
| Recovery-key group length, checksum algorithm and lower-case rule are not stated (`vault_format.md` open point 2) | Owner / reviewer (E-29/c01) | open | Medium |
| Passphrase Unicode normalisation before Argon2id is not stated; without one rule the same passphrase may not unlock on another platform | Owner / reviewer | open | Medium |
