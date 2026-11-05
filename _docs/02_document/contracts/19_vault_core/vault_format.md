# Contract: vault_format — vault header, file envelope and chunk stream bytes [v1.2]

**Component**: 19_vault_core (crate `vault-core` owns the layout; 20 and the tree store it as opaque bytes)
**Producer task**: E-29/c06 — `_docs/02_tasks/todo/B13-25_vault_header_format.md` (vault header, bounds); contributor: E-29/c08 (`B13-27`, file header, wrapped file key, chunk stream, JSON form); frozen by E-29/c01 (`B13-20`); envelope size agreed by E-29/c03 (`B13-22`); keys assigned at Step 4
**Consumer tasks**: E-30/c01 (header cap default, B11-16), E-30/c02..c06 (opaque header storage and routes), E-13 owner (bound of `revisions.client_meta`, through E-29/c03), E-26/n01 (web vault page), E-29/c12 (vector oracle), E-29/c13 (fuzz targets), the external reviewer (E-29/c15)
**Version**: 1.0.0
**Status**: draft (proposal for the external review; frozen only by E-29/c01; any later change bumps `format_version`)
**Last Updated**: 2026-11-04

## Purpose

The server stores the vault header (20) and each revision's wrapped file key (`revisions.client_meta`, 02) without parsing them, and the reviewer and the independent vector oracle must rebuild every byte from a written description. This file is that description. 19 owns it; 20 and 02 only need its sizes.

Sources: `components/19_vault_core/description.md` §3 (construction), §11; `components/19_vault_core/tests.md` IT-19-1, IT-19-4, IT-19-5, IT-19-7, IT-19-8, IT-19-11; `restrictions.md` RESTRICT-23; `data_model.md` (`revisions.client_meta`).

## Shape

### Vault header (stored by 20 as opaque bytes)

| Field | Width (proposal) | Authenticated | Rule |
|---|---|---|---|
| `format_version` | 1 byte | yes (AAD) | known versions only (1 for this contract); others `UnknownVersion` |
| `generation` | u64 | yes (AAD) | starts at 1; `rewrap_header` adds 1; never wraps |
| `vault_id` | 16 bytes (UUID) | yes (AAD) | the vault id of 20; part of the key-commitment input |
| `m_cost` | u32, KiB | yes (AAD) | 65,536 (64 MiB) to the platform cap (native 524,288 = 512 MiB; WASM and mobile caps from E-29/c02) |
| `t_cost` | u32 | yes (AAD) | 3 to 10 |
| `p_cost` | u32 | yes (AAD) | exactly 1 |
| `salt` | 16 bytes | yes (AAD) | random; any other length is malformed |
| wrapped vault key (passphrase) | 24-byte nonce + 32-byte ciphertext + 16-byte tag | is the AEAD output | XChaCha20-Poly1305 under the Argon2id KEK; AAD = every field above |
| wrapped vault key (recovery) | 24 + 32 + 16 bytes | is the AEAD output | XChaCha20-Poly1305 under a key derived from the recovery key (no Argon2); same AAD |
| key commitment | 32 bytes | checked after unwrap | keyed BLAKE3 under the vault key of `"sunnycloud vault commit v1" || vault_id` |

Proposal length with these widths: 229 bytes. Field order, byte order and the exact length are frozen by E-29/c01 and feed 20's `max_vault_header_bytes` (E-30/c01).

### Recovery key display

256-bit random value shown once as grouped base32 with a checksum; group length, checksum algorithm and case rule: Open point 2.

### File envelope (per revision; stored in `revisions.client_meta`)

| Part | Content | Rule |
|---|---|---|
| file header | `format_version`, `chunk_size`, `plaintext_len`, 19-byte random nonce prefix | `chunk_size` 64 KiB to 4 MiB; `plaintext_len` at most the caller's `max_file_bytes` |
| wrapped file key | 24-byte nonce + 32-byte ciphertext + 16-byte tag | XChaCha20-Poly1305 under the vault key; AAD = `vault_id || file_id || format_version`; a fresh random file key per revision |
| JSON form | an object carrying both parts with a fixed encoding | constant length; field names and encoding recorded by E-29/c03 together with the `client_meta` bound |

### Chunk stream (the uploaded ciphertext)

| Rule | Value |
|---|---|
| Construction | XChaCha20-Poly1305 STREAM, BE32 counter (`aead-stream =0.6.0`), nonce = 19-byte prefix, 4-byte big-endian counter, 1-byte last-chunk flag |
| Chunk AAD | `vault_id || file_id || format_version || chunk_size || plaintext_len` |
| Lengths | every chunk but the last holds exactly `chunk_size` plaintext bytes; each ciphertext chunk = plaintext + 16-byte tag; an empty file is one final chunk holding only the tag |
| End | the last chunk carries the final flag; the declared `plaintext_len` must match |

## Invariants

- Changing any byte of the vault header makes `unlock` fail (0 of 10,000 fuzzed mutations accepted, AC-61); bound fields fail with `ParamsOutOfBounds{field}` before any Argon2 work.
- A wrapped file key or ciphertext moved to another file id or another vault fails at unwrap or at chunk 0 (1,000 of 1,000 swaps, AC-60); the parent folder is not bound, so a move inside one vault still opens.
- Ciphertext length is a pure function of `plaintext_len` and `chunk_size`.
- The server never parses, alters or derives anything from these bytes (20 §1).
- Every change to this file after the freeze bumps `format_version` and regenerates the vector file through a reviewed commit (IT-19-1).

## Non-Goals

- Not covered: the API of the crate (`vault_core_api.md`); the HTTP wire encoding of the header (20 open point 1).
- Not covered: name encryption (data model A6, undecided); share-link keys (not in v1.2).

## Versioning Rules

- **Breaking**: any byte-level change; it is a new `format_version`, and readers keep accepting only the versions they know.
- **Non-breaking**: none (the layout has no optional fields).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| header-offsets | every byte of a generation-1 header xor 0x01, xor 0xFF, set to 0x00 | 0 accepted; bound fields typed, others authentication failures | IT-19-4 |
| bounds-table | m 63/64/512/513 MiB, t 2/3/10/11, p 0/1/2, salt 15/17 bytes, version 0/2/255 | only 64 MiB, 512 MiB (native), t 3 and 10, p 1, 16-byte salt and version 1 accepted | IT-19-5, FT-N-16 |
| envelope-json | serialise and parse 1,000 envelopes | byte-equal round trip, constant length | IT-19-7 |
| empty-file | 0-byte plaintext | one final chunk of 16 bytes | IT-19-2 |
| oracle | the independent Python oracle builds headers, envelopes and streams from this file alone | equal to the crate's bytes | IT-19-1 |

## Open points

1. Field order, byte order and the exact header length are a proposal until E-29/c01 freezes them with the reviewer.
2. Recovery-key display: group length, checksum algorithm and case rule.
3. Whether the vault header also carries a chunk size (IT-19-5, FT-N-16 bound it as a header field; 19 §3 lists it only in the file header).
4. The JSON field names and encoding of the envelope in `client_meta`, and the CHECK bound that must hold them (IT-19-7; E-29/c03 with the E-13 owner).
5. The WASM and mobile memory caps (E-29/c02).
6. If the review rejects `aead-stream`, libsodium `secretstream` replaces the chunk stream and `format_version` changes (R23).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B13) |
