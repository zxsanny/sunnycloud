# 19 Vault Core  [v1.2 — stub-level spec]

> **Release gate**: nothing in this component ships in v1.0 or v1.1 (FT-P-73). It is built, fuzzed and reviewed for v1.2 and **released only after the external review of the construction passes** (AC-62, restriction "Encryption and vault"). The format below is a design proposal for that review, not a frozen format; every "decision" can change at review.

## 1. High-Level Overview

**Purpose**: The client-side cryptography of the end-to-end encrypted vault, written once in Rust and compiled to native, WebAssembly and UniFFI (Kotlin and Swift bindings). It creates and unlocks a vault, wraps and unwraps per-file keys, and encrypts/decrypts file content in chunks. The server never links this crate. One responsibility: *the vault's cryptographic format and its validation rules*, with no I/O, no network and no UI.

**Architectural pattern**: Pure library (`vault-core`, `no_std + alloc`-friendly, no global state) with an incremental chunk API so memory stays at about one chunk plus Argon2 memory. Randomness is injected (`RandomSource` trait / caller-supplied bytes) because `getrandom` does not build for `wasm32-unknown-unknown` without a backend.

**Upstream dependencies**: none of the sunnycloud components and none of the `contracts` ports (so the WASM stays small and auditable). Crates (exact pins, `Cargo.lock` committed): `chacha20poly1305 =0.11.0` (XChaCha20-Poly1305, `default-features = false`, `alloc`), `aead-stream =0.6.0` (STREAM, **unaudited, 2026 initial release**), `argon2 =0.6.0` (`hash_password_into`, no `parallel`), `zeroize`, a keyed hash for commitment and key derivation (`blake3` 1.8.7, already in the workspace), `zxcvbn` (feature `passphrase-policy`, size check for WASM), a fixed 7,776-word list for generated passphrases.
**Downstream consumers**: 16_web_app (WASM in a module Web Worker, single-threaded), 17_android_app and 18_ios_app (UniFFI bindings inside the same library as 13, one library per ABI), 14_desktop_daemon only if desktop vault access is added (not specified; vault folders sync as ciphertext without it).

**Scope in**: header format and generation counter, bounds validation, KDF, wrapping, key commitment, recovery key, per-file chunk encryption/decryption, AAD binding, passphrase policy and generation, zeroization, build targets.
**Scope out**: server storage of the opaque header (20), the per-revision wrapped file key (stored by 02 in `revisions.client_meta`, an opaque field), server-side vault limits, vault-aware listing: these belong to 20 and the tree/transfer components. Vault files cannot be link-shared in v1.2, so there is no share-link key operation. 19 is the client crypto crate only; no server component links it. Refusal of camera backup into a vault and UI (16/17); rate limits on unlock attempts (client side, open question 10).

## 2. Interfaces

### Ports

19 provides and requires no `contracts` ports and has no sunnycloud dependency: it is a pure library with the operations below. Its server counterpart is `20_vault_service`, which stores and returns the bytes this crate produces and never parses them.

### Operations (proposed)

| Operation | Input | Output | Errors (`VaultError`) |
|---|---|---|---|
| `check_passphrase` | passphrase | `Ok` | `TooShort` (< 12 characters), `TooWeak` (zxcvbn score < 4) |
| `generate_passphrase` | `RandomSource` | 6 words from the fixed 7,776-word list (about 77 bits), always accepted by `check_passphrase` | `Rng` |
| `create_vault` | passphrase, `KdfParams` (default 64 MiB, t=3, p=1), `vault_id`, `RandomSource` | `NewVault{header_bytes, recovery_key (shown once), UnlockedVault}` | `Passphrase*`, `ParamsOutOfBounds`, `Rng` |
| `unlock` | `header_bytes`, passphrase, `pinned_generation: Option<u64>` | `UnlockedVault` (carries `generation`) | `HeaderMalformed`, `UnknownVersion`, `ParamsOutOfBounds{field}`, `HeaderAuthFailed`, `WrongPassphraseOrTampered`, `KeyCommitmentMismatch`, `HeaderRolledBack{seen, pinned}` |
| `unlock_with_recovery` | `header_bytes`, recovery key | `UnlockedVault` | same family plus `RecoveryKeyInvalid` |
| `rewrap_header` | `UnlockedVault`, new passphrase | new `header_bytes` with `generation + 1` | as `create_vault` |
| `begin_encrypt` | `UnlockedVault`, `file_id`, `plaintext_len`, `RandomSource` | `FileEncryptor` + `FileEnvelope{header, wrapped_file_key}` (a fresh file key per revision) | `TooLarge{limit}` (100 MiB configurable), `Rng` |
| `FileEncryptor::next_chunk` / `finish` | plaintext chunk (64 KiB to 4 MiB, the declared chunk size) | ciphertext chunk (+16 B tag); `finish` marks the final chunk | `ChunkSize`, `LengthMismatch` |
| `begin_decrypt` | `UnlockedVault`, `file_id`, `FileEnvelope` | `FileDecryptor` | `WrappedKeyRejected` (wrong vault or file), `UnknownVersion`, `HeaderAuthFailed` |
| `FileDecryptor::next_chunk` / `finish` | ciphertext chunk | plaintext chunk | `ChunkAuthFailed{index}`, `Truncated`, `Reordered`, `TrailingData` |
| `lock` | `UnlockedVault` | zeroizes all key material | none |

`UnlockedVault` is move-only, `!Clone`, no `Debug` output of secrets, `Drop` zeroizes. UniFFI and WASM adapters call block-sized methods only so the Worker heap stays bounded (AC-14: 250 MB peak on iOS Safari and 4 GB Android).

## 3. Format and construction (proposal for the review)
- **KEK**: `Argon2id` v0x13 from the passphrase and a 16-byte random salt, 32-byte output. Header stores `(version, generation, m_cost KiB, t_cost, p_cost, salt)`; defaults m=64 MiB, t=3, p=1 (deliberate deviation from RFC 9106's p=4: WASM has no threads; same memory and passes). **The KDF parameters come from an untrusted header, so they are bounded before any Argon2 work**: `t` in 3..=10, `m` from 64 MiB up to the platform cap (512 MiB native; the mobile and WASM caps are set by the S7 benchmark), **p = 1**, chunk size 64 KiB to 4 MiB, known `format_version` only (AC-61). Anything outside is `ParamsOutOfBounds{field}` (NFT-SEC-41 sends `t = u32::MAX` and `m` above the cap and requires an immediate error with no Argon2 run). Unlock <= 1 s native, <= 2 s browser (S7 benchmark on a low-end phone and Safari decides the defaults).
- **Vault key** (random 256 bit) wrapped twice: under the KEK and under a key derived from the **recovery key** (random 256 bit, shown once as grouped base32 with a checksum; high entropy so no Argon2 for it). Wrapping is XChaCha20-Poly1305 with a random 192-bit nonce.
- **Header authentication and rollback**: the wrapping AEAD takes the *entire serialised header (all fields except the wrapped blobs, including `generation`)* as AAD, so changing any header byte fails unwrapping (0 of 10,000 fuzzed mutations accepted). After unwrap the client also checks the **key-commitment value**, a keyed BLAKE3 of `("sunnycloud vault commit v1" || vault_id)` under the vault key stored in the header, because XChaCha20-Poly1305 alone is not key-committing. `generation` starts at 1 and `rewrap_header` increments it; clients store the last generation they saw per vault (`pinned_generation`) and `unlock` returns `HeaderRolledBack` when the header is older. The pin protects a returning client against a server that serves an old header (for example one with a revoked passphrase); a device that has never seen the vault has nothing to pin, which is a stated limit.
- **Per file**: random 256-bit file key per revision, wrapped by the vault key with AAD = `vault_id || file_id || format_version` (the parent is **not** bound, so a move is a pure tree change with no re-wrap). The wrapped key is stored by the server inside that revision's `revisions.client_meta`; the file header carries chunk size, plaintext length and nonce prefix. Chunk AAD binds `vault_id || file_id || format_version || chunk_size || plaintext_len`. Swapping a wrapped key and ciphertext into another file or vault fails at unwrap or at the first chunk (AC-60: 1,000 of 1,000 swaps). **Consequence for review**: the folder tree, file names, which revision is current and the parent of a file are chosen by the server and not authenticated here; a malicious server can move, hide or roll back files inside a vault but cannot read or alter content.
- **Content**: XChaCha20-Poly1305 STREAM (`aead-stream` BE32, 19-byte random nonce prefix), plaintext chunk size fixed per file in the authenticated file header; the **plaintext length (or chunk count) is in the authenticated header** so truncation is detected, and the last chunk carries the final flag. Empty files and exact multiples of the chunk size round-trip (FT-P-46). The server's own at-rest encryption (04) sits on top and is unrelated.
- **Names**: whether file names are ciphertext is open (data model 10.2); this crate offers `seal_name`/`open_name` only if the review decides yes.
- **Passphrase and offline guessing**: the header is stored on the server, so whoever holds the database (the operator, a thief, a backup) can guess passphrases offline at the speed Argon2id at 64 MiB and t=3 allows; nothing rate-limits that. A passphrase must score 4 on zxcvbn (an estimator that does not depend on any server rule) or be generated by `generate_passphrase`; a user-chosen passphrase that passes is still only as strong as its entropy and not a guarantee against a funded attacker. The recovery key has full entropy. The product text states this residual risk (NFT-SEC-43).
- **Fallback**: libsodium `secretstream` (audited) if the reviewer rejects `aead-stream`; wire format would change, so `format_version` exists from day one.

## 4. Data, side effects, dependencies
No data owned, no I/O, no logging of material. Side effects: allocation and zeroization only. **Implement after**: the external review plan is agreed and the format fixed; helper 02 is not needed. **Parallel with**: `20_vault_service`, 16 vault page. **Blocks**: 16/17/18 vault screens, FT-P-44 to FT-P-47.

## 5. Build targets
| Target | Tooling | Notes |
|---|---|---|
| Native (tests, desktop, server-side test tools) | cargo | OS RNG feature on |
| WebAssembly | `wasm-bindgen =0.2.130`, `wasm-pack` 0.15.x, `--target web`, module Worker, single-threaded | no threads, no COOP/COEP; RNG bytes from `crypto.getRandomValues`; core measured at about 60 KB with zero imports; Vite + SvelteKit glue needs a spike with a 100 MiB file |
| Mobile | UniFFI bindings (Kotlin, Swift; version pinned at implementation, **[Verify at pin]**) compiled **into the same library as 13** (assembled by 17); no flutter_rust_bridge (S4-4) | block-sized calls; the native wrapper owns the `UnlockedVault` handle; NDK r28+, 16 KB alignment check in CI |
Same code, same test vectors on all three (native, WASM, UniFFI); the vector file is a CI gate.

## 6. Configuration
Compile-time/format constants, not operator config: `format_version`, KDF bounds, chunk-size bounds. Runtime parameters supplied by the caller: `max_file_bytes` (100 MiB default, from server limits), KDF params chosen at creation. Durations: none.

## 7. Security notes
Wrong-passphrase and tamper errors are distinguishable only where the construction allows it (do not leak which header field failed beyond the typed bound errors); constant-time comparisons for commitment checks; no secret in `Debug`, logs, panics or error text; zeroize on lock and on drop; WASM linear memory cannot be guaranteed wiped, so the web page terminates the Worker on lock (heap scan finds no key, AC-62). Randomness never comes from `Math.random`. Unlock attempts are limited client side (open question 10); this does not limit offline guessing against a stolen header (section 3). Metadata exposure (sizes, counts, tree shape, timing) is stated in the product text (NFT-SEC-43).

## 8. ACs implemented
| AC | Release | Part |
|---|---|---|
| AC-13 | v1.2 | server sees ciphertext only; tampered, truncated, reordered, wrong-vault data rejected |
| AC-14 | v1.2 | 100 MiB files, bounded memory, unlock time |
| AC-60 | v1.2 | AAD binding, 1,000 of 1,000 swaps rejected, empty/exact-multiple files, unknown versions rejected |
| AC-61 | v1.2 | header bounds (`t`, `m`, `p`, chunk size) and authentication, generation pin, 0 of 10,000 mutations accepted |
| AC-62 | v1.2 | zeroize on lock; external review gate |
**Interaction ACs folded in**: none are assigned (F17 is not in AC-68 to AC-80). Vault files cannot be link-shared in v1.2 (canonical-names), so the F11 x F17 seam does not exist in this release.

## 9. Tests
FT-N-13, FT-N-14, FT-N-15, FT-N-16, FT-N-37, FT-P-44, FT-P-46, FT-P-47, NFT-SEC-39, NFT-SEC-40, NFT-SEC-41, NFT-SEC-42, NFT-SEC-43, NFT-PERF-29, NFT-RES-LIM-08. Unit: known-answer vectors, 10,000-case header mutation fuzz, header bounds table including `t = 2`, `t = 11`, `m` above the cap (no Argon2 run), generation rollback, passphrase policy (score 3 rejected, score 4 and generated accepted), 1,000 random swaps, truncate/reorder/append at every chunk index, final-flag cases, bounds table. cargo-fuzz targets for the header parser and chunk decryptor; the same corpus runs in V8 (wasm).

## 10. Component acceptance
All of section 9's vectors pass on native, WASM and UniFFI with identical outputs; the external review report has no unresolved findings; unlock time and 100 MiB memory peaks meet AC-14 on the reference browsers/phones; after `lock`, a heap scan of the Worker finds no key bytes.

## 11. Risks and open points
1. **Review gate** can change the format; `aead-stream` has no audit and `chacha20poly1305`'s 2020 audit may not cover XChaCha (verify). Libsodium fallback costs a C toolchain on WASM/Android/iOS.
2. Argon2 timing on low-end phones and Safari is unmeasured; parameters may drop to the 64 MiB floor or rise only where the 1 s/2 s budget holds.
3. The parent is bound in neither the key-wrap nor the chunk AAD, so the requirement text ("AAD binds vault, file and parent") is met only for vault and file; the tree is server-controlled and unauthenticated. The reviewer must accept this, or the requirement is changed.
4. The server side is `20_vault_service`; the two must agree on the opaque header and `FileEnvelope` byte layout (19 owns the layout, 20 stores it as bytes).
5. zxcvbn dictionary and the 7,776-word list in WASM size; if too large, the web page uses `zxcvbn-ts` and only the length rule lives here (then the rule exists twice).
6. Name encryption and per-vault versus per-account vault count are undecided (data model A6).
7. Vault files cannot be link-shared in v1.2 (owner decision, canonical-names); a later release would need a link-key design and its own review.
8. The wrapped file key travels in `revisions.client_meta` of the revision, so it commits atomically with the revision through the normal commit path; no separate key call exists (resolves the former atomic-commit open item).
