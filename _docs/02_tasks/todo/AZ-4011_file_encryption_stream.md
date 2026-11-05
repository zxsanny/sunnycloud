# Per-file encryption: `begin_encrypt`/`begin_decrypt`, STREAM chunks, AAD binding, wrapped file key [v1.2]

**Task**: AZ-4011_file_encryption_stream
**Name**: Vault file encryption
**Description**: Encrypt and decrypt vault files chunk by chunk with a fresh file key per revision wrapped under the vault key, bind key and chunks to vault, file and format version (not the parent), authenticate length and final chunk, and refuse files above the size limit
**Complexity**: 5 points
**Dependencies**: AZ-4006_envelope_client_meta_spike, AZ-4009_vault_header_format, AZ-4010_vault_create_unlock
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4011
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — `begin_encrypt`, `FileEncryptor`, `begin_decrypt`, `FileDecryptor`, `FileEnvelope` (this task contributes them).
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — file header, wrapped file key, chunk stream, JSON form (this task contributes them).
- Record of E-29/c03 (envelope JSON form and the `client_meta` bound).

## Problem

The server must see only ciphertext, and a client must reject tampered, truncated, reordered or wrong-vault data (AC-13). A wrapped key and ciphertext moved to another file or vault must fail 1,000 of 1,000 times, while a move to another folder of the same vault still opens, because the parent is deliberately not bound (AC-60 as reworded at Step 5, R35). Memory must stay near one chunk for 100 MiB files (AC-14), and files above 100 MiB (configurable) are refused with a clear limit (FT-N-14).

## Outcome

- `begin_encrypt(vault, file_id, plaintext_len, RandomSource)`: a fresh random 256-bit file key per call, wrapped by the vault key with AAD `vault_id || file_id || format_version` and a random 192-bit nonce; a file header with chunk size, plaintext length and a 19-byte random nonce prefix; `TooLarge{limit}` above `max_file_bytes`.
- `FileEncryptor::next_chunk`/`finish`: XChaCha20-Poly1305 STREAM (BE32) with chunk AAD `vault_id || file_id || format_version || chunk_size || plaintext_len`; final flag on the last chunk; `ChunkSize` and `LengthMismatch`.
- `begin_decrypt` and `FileDecryptor::next_chunk`/`finish`: `WrappedKeyRejected`, `UnknownVersion`, `HeaderAuthFailed`, `ChunkAuthFailed{index}`, `Truncated`, `Reordered`, `TrailingData`; `finish` never Ok on an altered stream.
- `FileEnvelope` serialised to the JSON form of `vault_format.md` with constant length.

## Scope

### Included
- The operations above, the envelope JSON form, the size limit (caller-supplied `max_file_bytes`, default 104,857,600).
- Tests IT-19-2, IT-19-3, IT-19-7, IT-19-11, IT-19-12.

### Excluded
- Upload of the ciphertext and the commit of `client_meta` (client pages, E-26/n01; server paths).
- Zeroization rules of the encryptor and decryptor handles (E-29/c09).

## Acceptance Criteria

**AC-1: Edge sizes round-trip (IT-19-2, FT-P-46, AC-60)**
Given chunk sizes 64 KiB and 4 MiB and plaintext sizes 0, 1, chunk - 1, chunk, chunk + 1, 2 chunks, 16 chunks, 16 chunks + 1, plus the 100 MiB DS-18 file
When each is encrypted and decrypted with a freshly unlocked vault
Then all 17 round-trip with equal BLAKE3, and ciphertext length = plaintext length + 16 bytes per chunk (an empty file is one final chunk of 16 bytes)

**AC-2: Tampering never yields a complete file (IT-19-3, FT-N-13, AC-13)**
Given a 17-chunk file at 64 KiB
When for every chunk index one byte is flipped, the chunk is swapped with every other (136 pairs), dropped or duplicated, and the whole-file cases are applied (last chunk or last byte removed, first chunk removed, a valid next chunk or 1 byte appended, final flag cleared on the last chunk or set on chunk 0, a chunk of another file under the same key inserted)
Then flips, swaps, drops, duplicates and foreign chunks give `ChunkAuthFailed{index}` at the first differing index, removals and the cleared flag give `Truncated` from `finish`, appends give `TrailingData`, `finish` is never Ok, and plaintext returned before the failing index equals the original

**AC-3: Fresh key per revision (IT-19-7)**
Given one file id and 1,000 `begin_encrypt` calls of the same plaintext with real randomness
When revision n's ciphertext is opened with revision m's envelope for 1,000 random pairs (n != m)
Then all file keys and nonce prefixes differ, all 1,000 cross-revision opens fail with `ChunkAuthFailed{index: 0}`, same-revision opens succeed, and the envelope JSON round-trips byte-equal at constant length

**AC-4: Swaps rejected, moves allowed (IT-19-11, FT-N-15, NFT-SEC-40, AC-60)**
Given 3 vaults with 5 files each and 1,000 seeded swaps (envelope and ciphertext of B opened as A, same and across vaults, wrapped key only, ciphertext only, an envelope of V2 opened with V1's vault, format version byte 0, 2, 255)
When each is opened
Then 1,000 of 1,000 are rejected (`WrappedKeyRejected`, `ChunkAuthFailed{index: 0}` or `UnknownVersion`), and the same file opened after a move to another folder of its vault decrypts with equal hash

**AC-5: Size limit and chunk contract (IT-19-12, FT-N-14, AC-14)**
Given the default limit
When `begin_encrypt` is called with 104,857,600 bytes and with 104,857,601 bytes, then with `max_file_bytes` 200 MiB for 101 MiB, and `next_chunk` gets a chunk 1 byte over the declared size, a short non-final chunk, and totals shorter or longer than declared
Then 100 MiB is accepted, 100 MiB + 1 returns `TooLarge{limit: 104857600}`, 101 MiB is accepted under 200 MiB, oversize and undersize non-final chunks return `ChunkSize`, wrong totals return `LengthMismatch` from `finish`, and no ciphertext is emitted after an error

## Non-Functional Requirements

**Performance**
- Peak allocation above the unlocked baseline at most 3 chunks (12 MiB at 4 MiB chunks) for 100 MiB (measured in E-29/c14, PT-19-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | 17 size cases at both chunk sizes | Round-trip; length rule |
| AC-2 | Every-index and whole-file tamper cases | Typed failure at the right index; `finish` never Ok |
| AC-3 | 1,000 revisions; 1,000 cross opens; JSON round trip | Distinct; rejected; byte-equal |
| AC-4 | 1,000 seeded swaps; same-vault move | 1,000 rejected; move opens |
| AC-5 | Size limit and chunk-contract cases | Typed errors; no output after error |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | WASM build in Node (300 s budget) | Size cases | Round-trip | FT-P-46 |
| AC-2 | WASM build in Node | Tamper cases | Same results as native | FT-N-13, NFT-SEC-39 |
| AC-4 | Kotlin build on a JVM | Swap set | 1,000 rejected | FT-N-15, NFT-SEC-40 |

## Constraints

- The parent folder is not part of any AAD (owner decision at Step 5); moves inside a vault need no re-wrap.

## Runtime Completeness

- **Capability**: XChaCha20-Poly1305 STREAM BE32 (`aead-stream =0.6.0`, or libsodium `secretstream` if E-29/c01 decides the fallback).
- **Production code that must exist**: the real chunk stream and key wrap in every build.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: whole-file AEAD, an unauthenticated length, a home-made nonce scheme.

## Risks & Mitigation

**Risk 1: Nonce handling error in STREAM (same class as R08)**
- *Risk*: a counter or prefix mistake reuses a nonce.
- *Mitigation*: known-answer vectors (E-29/c12), fuzzing (E-29/c13), nonce-uniqueness test ST-19-4 (E-29/c14), external review.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Who chooses a file's chunk size (caller argument or vault default) is not stated (`vault_core_api.md` open point 4) | Owner / reviewer | open | Medium |
| The server can move, hide or roll back vault files (parent and current revision not authenticated, R35); stated in product text, accepted by the reviewer or the requirement changes | Owner / reviewer | accepted-risk | Medium |
