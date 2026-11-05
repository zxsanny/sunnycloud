# Contract: vault_core_api — `vault-core` operations, types, errors and the WASM and UniFFI forms [v1.2]

**Component**: 19_vault_core (crate `vault-core`; native, WebAssembly and UniFFI builds)
**Producer task**: E-29/c04 — `_docs/02_tasks/todo/B13-23_vault_core_skeleton.md` (`RandomSource`, `VaultError`, constants, build targets); contributors: E-29/c05 (`B13-24`, passphrase policy), E-29/c07 (`B13-26`, create, unlock, rewrap), E-29/c08 (`B13-27`, file encryption), E-29/c09 (`B13-28`, lock and handle rules), E-29/c10 (`B13-29`, WASM exports), E-29/c11 (`B13-30`, UniFFI surface); keys assigned at Step 4
**Consumer tasks**: E-26/n01 (web vault page, written by batch B12; WASM in a module Web Worker), E-29/c12, c13, c14, c15; pipeline AZ-3675; mobile vault callers in 17/18 are not planned (flagged on E-29/c11 and AZ-3675)
**Version**: 1.0.0
**Status**: draft (the construction is a proposal for the external review; frozen by E-29/c01, changed only by a reviewed format bump)
**Last Updated**: 2026-11-04

## Purpose

The one client-side vault cryptography library (ADR-007, ADR-001, RESTRICT-23). The web vault page calls it through WASM; mobile hosts would call it through UniFFI inside the same library as 13. The server never links it. Callers read this file for the operations, errors and caller duties; the byte layout is in `vault_format.md`.

Sources: `components/19_vault_core/description.md` §2, §3, §5, §6, §7; `components/19_vault_core/tests.md`; `epics/E-29_vault_core.md`; `acceptance_criteria.md` AC-13, AC-14, AC-60, AC-61, AC-62; `restrictions.md` RESTRICT-22, RESTRICT-23, RESTRICT-33.

## Shape

### Operations (Rust API; all pure, no I/O, no network, no logging)

| Operation | Signature (indicative) | Errors (`VaultError`) | Blocking? |
|---|---|---|---|
| `check_passphrase` | `(passphrase) -> Result<(), VaultError>` | `TooShort` (< 12 characters), `TooWeak` (zxcvbn score < 4) | sync |
| `generate_passphrase` | `(&mut dyn RandomSource) -> String` (6 words, fixed 7,776-word list) | `Rng` | sync |
| `create_vault` | `(passphrase, KdfParams, vault_id, &mut dyn RandomSource) -> NewVault{header_bytes, recovery_key, vault: UnlockedVault}` | `TooShort`, `TooWeak`, `ParamsOutOfBounds{field}`, `Rng` | sync (one Argon2id derivation) |
| `unlock` | `(header_bytes, passphrase, pinned_generation: Option<u64>) -> UnlockedVault` | `HeaderMalformed`, `UnknownVersion`, `ParamsOutOfBounds{field}`, `HeaderAuthFailed`, `WrongPassphraseOrTampered`, `KeyCommitmentMismatch`, `HeaderRolledBack{seen, pinned}` | sync |
| `unlock_with_recovery` | `(header_bytes, recovery_key) -> UnlockedVault` | the `unlock` family plus `RecoveryKeyInvalid` | sync (no Argon2) |
| `rewrap_header` | `(&UnlockedVault, new_passphrase, &mut dyn RandomSource) -> header_bytes` (generation + 1) | as `create_vault`; generation overflow (variant name open, see Open points) | sync |
| `begin_encrypt` | `(&UnlockedVault, file_id, plaintext_len, &mut dyn RandomSource) -> (FileEncryptor, FileEnvelope)`; chunk size choice: Open point 4 | `TooLarge{limit}`, `Rng` | sync |
| `FileEncryptor::next_chunk` / `finish` | `(plaintext chunk) -> ciphertext chunk`; `finish() -> last ciphertext chunk` | `ChunkSize`, `LengthMismatch` | sync |
| `begin_decrypt` | `(&UnlockedVault, file_id, &FileEnvelope) -> FileDecryptor` | `WrappedKeyRejected`, `UnknownVersion`, `HeaderAuthFailed` | sync |
| `FileDecryptor::next_chunk` / `finish` | `(ciphertext chunk) -> plaintext chunk`; `finish() -> Result<(), VaultError>` | `ChunkAuthFailed{index}`, `Truncated`, `Reordered`, `TrailingData` | sync |
| `lock` | `(UnlockedVault)` (consumes it) | none | sync |

### Types

| Type | Shape | Rule |
|---|---|---|
| `KdfParams` | `{m_cost_kib, t_cost, p_cost}` | default 65,536 KiB (64 MiB), t = 3, p = 1 |
| `UnlockedVault` | opaque handle carrying `vault_id` and `generation` | move-only, not `Clone`, no secret in `Debug`, zeroized on `lock` and on drop |
| `NewVault` | `{header_bytes, recovery_key, vault}` | `recovery_key` is returned once and never kept by the crate |
| `RecoveryKey` | 256-bit random value; display form grouped base32 with a checksum | display rules: `vault_format.md` |
| `FileEnvelope` | `{header, wrapped_file_key}`; JSON form for `revisions.client_meta` | layout and JSON form: `vault_format.md` |
| `RandomSource` | trait: fill a caller buffer with random bytes or fail with `Rng` | every key, salt and nonce comes from it; native builds have an OS source; WASM callers pass bytes from `crypto.getRandomValues` |
| `max_file_bytes` | caller-supplied limit | default 104,857,600 bytes (100 MiB) |

`VaultError` (complete set for 1.0.0): `TooShort`, `TooWeak`, `Rng`, `ParamsOutOfBounds{field}`, `HeaderMalformed`, `UnknownVersion`, `HeaderAuthFailed`, `WrongPassphraseOrTampered`, `KeyCommitmentMismatch`, `HeaderRolledBack{seen, pinned}`, `RecoveryKeyInvalid`, `TooLarge{limit}`, `ChunkSize`, `LengthMismatch`, `WrappedKeyRejected`, `ChunkAuthFailed{index}`, `Truncated`, `Reordered`, `TrailingData`, plus the generation-overflow variant (Open points). No variant carries a secret, a passphrase or a header field value beyond the named bound field.

### WASM form (E-29/c10; `wasm-bindgen =0.2.130`, `--target web`)

| Rule | Value |
|---|---|
| Host | a module Web Worker, single-threaded; no threads, no COOP/COEP |
| Calls | one export per operation above; content is passed one chunk per call (block-sized), never a whole file |
| Randomness | the glue fills buffers from `crypto.getRandomValues`; never `Math.random`; the core module itself imports nothing for randomness |
| Handles | `UnlockedVault`, `FileEncryptor`, `FileDecryptor` are JS objects that must be freed; `lock` frees and zeroizes |
| Errors | thrown as a typed error object carrying the `VaultError` name and its non-secret fields |
| Caller duty | the page terminates the Worker after `lock`, because WASM linear memory cannot be guaranteed wiped (AC-62, NFT-SEC-42) |

### UniFFI form (E-29/c11; UniFFI 0.32.x, the workspace version)

| Rule | Value |
|---|---|
| Library | compiled into the same mobile library as 13 and `mobile_backup` (17 assembles it), only in v1.2 builds |
| Objects | `UnlockedVault`, `FileEncryptor`, `FileDecryptor` as UniFFI objects; the native wrapper owns the handle |
| Calls | block-sized only, as in WASM |
| Errors | `VaultError` as a UniFFI error enum, same names |
| Randomness | foreign `RandomSource` callback or the platform OS source (Open points) |

## Invariants

- Header-supplied KDF parameters, chunk size and format version are checked before any Argon2 work: a rejected header runs 0 derivations, allocates under 8 MiB and returns within 50 ms (AC-61, IT-19-5).
- The same inputs and the same `RandomSource` bytes give byte-identical outputs on native, WASM and UniFFI (IT-19-1).
- Every wrap and content nonce comes from `RandomSource`; a failing source returns `Rng` and emits no bytes (ST-19-4).
- A fresh file key per `begin_encrypt` call (per revision); the key-wrap AAD binds `vault_id`, `file_id` and the format version, not the parent folder, so a move inside one vault needs no re-wrap (AC-60 as reworded at Step 5).
- `FileDecryptor::finish` never returns Ok for an altered, truncated, reordered or extended stream (AC-13).
- `unlock` with `pinned_generation = Some(p)` refuses a header with `generation < p` (`HeaderRolledBack`); `None` accepts any generation (stated limit).
- Commitment, tag and checksum comparisons are constant-time.
- No secret appears in `Debug`, `Display`, panics, logs or error values (ST-19-1).
- Nothing in this crate is linked into v1.0 or v1.1 release builds (ADR-016, FT-P-73).

## Non-Goals

- Not covered: the byte layout (`vault_format.md`); storage of the header (E-30, `contracts/20_vault_service/vault_http_api.md`); the vault page UI, unlock attempt limits and product text placement (E-26/n01).
- Not covered: name encryption (`seal_name`/`open_name` exist only if the review decides names are ciphertext); share links for vault files (not in v1.2).

## Versioning Rules

- **Breaking**: an operation, type or `VaultError` variant removed or renamed, a signature changed, any output change for the same inputs (that is a format change: `vault_format.md` bumps `format_version`).
- **Non-breaking**: a new operation, a new optional parameter with the old default.
- While `Status` is draft (before E-29/c01 freezes the format), the shape may change without a major bump.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| vectors | `vectors/vault-v1.json` with its deterministic `RandomSource` | byte-identical on native, WASM, Kotlin (Swift at the release gate) | IT-19-1 |
| bounds-before-kdf | header with `t = u32::MAX`; `m` above the cap | `ParamsOutOfBounds{field}`, 0 derivations, < 50 ms | IT-19-5, NFT-SEC-41 |
| round-trip | sizes 0, 1, chunk - 1, chunk, chunk + 1, 2, 16, 16 chunks + 1 at 64 KiB and 4 MiB | equal BLAKE3 after decrypt | IT-19-2, FT-P-46 |
| tamper | flip, swap, drop, duplicate, append, final-flag cases | `ChunkAuthFailed{index}`, `Truncated` or `TrailingData`; never Ok | IT-19-3, FT-N-13 |
| swaps | 1,000 seeded swaps across files and vaults | 1,000 rejected; same-vault move opens | IT-19-11, FT-N-15 |
| rollback | header generation 1 with pin 2 | `HeaderRolledBack{seen: 1, pinned: 2}` | IT-19-8 |
| passphrase | 11 characters; score 3; score 4; generated | `TooShort`; `TooWeak`; Ok; Ok | IT-19-9, FT-N-37 |
| size-limit | 100 MiB + 1 with the default limit | `TooLarge{limit: 104857600}` | IT-19-12, FT-N-14 |

## Open points

1. `rewrap_header` on a header at `generation = u64::MAX` must fail without wrapping to 0; the `VaultError` variant is not named (IT-19-8).
2. Recovery-key input rules (case handling, separators, checksum algorithm, group length) are not stated beyond "grouped base32 with a checksum" (IT-19-10 requires one documented case rule).
3. Unicode normalisation of the passphrase before Argon2id is not stated; without one rule, the same passphrase typed on two platforms may give different bytes.
4. Whether chunk size is a field of the vault header (IT-19-5 and FT-N-16 bound it there) or only of the file header (19 §3) is not consistent, and who picks a file's chunk size (a caller argument or a vault default) is not stated; IT-19-2 needs both 64 KiB and 4 MiB files.
5. Mobile randomness source (foreign callback or OS source) is not stated.
6. The WASM and mobile Argon2 memory caps come from the S7 benchmark (E-29/c02).
7. If zxcvbn plus the word list is too large for WASM, the web page uses `zxcvbn-ts` and only the length rule lives here (19 risk 5); the passphrase rule would then exist twice.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B13) |
