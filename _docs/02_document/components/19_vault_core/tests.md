# Test Specification — 19 Vault Core [v1.2]

Crate-level tests of the client cryptography (`vault-core`). System scenarios that drive the same code through the real clients are referenced, not restated: FT-N-13 to FT-N-16, FT-N-37, FT-P-44 to FT-P-47, NFT-SEC-39 to NFT-SEC-43, NFT-PERF-29, NFT-RES-LIM-08. IDs here: IT (through the public API), PT, ST, AT.

**Where it runs**: IT, native PT and ST run on a developer workstation or CI runner (Rust unit tier, `cargo test -v`, no Docker). The same vector file and IT cases run on three targets: native, WebAssembly (`wasm-pack test --node` and headless Chromium) and UniFFI (Kotlin on a JVM; Swift on the owner Mac, `NOT-RUN(H-MAC)` elsewhere and blocking only for the release gate). Browser and phone timing is system tier (NFT-PERF-29) and is never measured on the workstation. **Status**: READY = defined by the component spec and an AC; BLOCKED names the missing piece.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-13 | Server sees only ciphertext; tampered, truncated, reordered or wrong-vault ciphertext rejected by the client | IT-19-1, -2, -3, -6, AT-19-1; FT-N-13, NFT-SEC-39 | Covered |
| AC-14 | 100 MiB files; unlock at most 1 s native, 2 s browser; bounded memory | IT-19-2, -12, PT-19-1, -2; NFT-PERF-29, NFT-RES-LIM-08 | Covered (phone and Safari numbers: system tier) |
| AC-60 | Swaps rejected 1,000 of 1,000; empty and exact-chunk files round-trip; unknown versions rejected | IT-19-2, -7, -11; FT-N-15, FT-P-46, NFT-SEC-40 | Covered (parent is not bound by design; owner decision at Step 5) |
| AC-61 | Header bounds, 0 of 10,000 mutations accepted | IT-19-4, -5, -8; FT-N-16, NFT-SEC-41 | Covered; mobile and WASM memory caps BLOCKED (IT-19-5) |
| AC-62 | No key in memory after lock; external review | ST-19-2, AT-19-2; NFT-SEC-42 | Covered; review is a manual gate |
| RESTRICT-33 | Passphrase of 12+ characters and zxcvbn score 4, or generated | IT-19-9; FT-N-37, NFT-SEC-41 | Covered |

RESTRICT-22 (recovery key) is covered by IT-19-10; RESTRICT-23 (construction) by IT-19-1 to -8 and ST-19-4.

## Blackbox Tests

### IT-19-1: Known-answer vectors agree on native, WASM and UniFFI

**Summary**: Every primitive and the full serialisation give the same bytes on all targets and as an independent implementation. **Traces to**: AC-13, AC-60, AC-61

**Description**: `vectors/vault-v1.json` is made once by an oracle sharing no code with the crate (Python: `argon2-cffi`, `PyNaCl` XChaCha20-Poly1305, `blake3`, and a short STREAM BE32 routine written from the spec); it is a committed fixture and a CI gate. A deterministic `RandomSource` replays the bytes listed in the file. Vectors: Argon2id v0x13 (m=64 MiB, t=3, p=1; 2 vectors) and the RFC 9106 vector (m=32 KiB, t=3, p=4) through the internal KDF function that sits behind the bounds check; the vault key wrapped under the KEK and under the recovery key (fixed 24-byte nonce, serialised header as AAD); the key commitment (keyed BLAKE3 over `"sunnycloud vault commit v1" || vault_id`); a file key wrapped with AAD `vault_id || file_id || format_version`; content for an empty file, one chunk, 150,000 bytes at 64 KiB chunks (65,536 + 65,536 + 18,928) and 3 chunks at 4 MiB (fixed 19-byte nonce prefix, SHA-256 of the ciphertext); full headers at generation 1 and 2.

**Expected result**: every output byte-identical to the file on native, WASM, Kotlin and (release gate) Swift; decrypting the vector ciphertext returns the vector plaintext. **Max time**: 60 s per target.

### IT-19-2: Round trip for edge sizes at both chunk sizes

**Summary**: Boundary-size files round-trip. **Traces to**: AC-13, AC-14, AC-60 (FT-P-46 at crate level)

**Description**: For chunk sizes 64 KiB and 4 MiB, plaintext sizes 0, 1, chunk-1, chunk, chunk+1, 2 chunks, 16 chunks, 16 chunks+1, plus the 100 MiB file of DS-18 through the block-sized API. Encrypt, then decrypt with a fresh `UnlockedVault`, compare BLAKE3.

**Expected result**: 17 of 17 round-trip with equal hash; ciphertext length = plaintext length + 16 bytes per chunk (an empty file is one final chunk, tag only). **Max time**: 120 s native, 300 s WASM.

### IT-19-3: Chunk tamper, reorder, truncation, append and final-flag cases

**Summary**: A decryptor never yields a complete-looking file from altered ciphertext. **Traces to**: AC-13 (FT-N-13 at crate level)

**Description**: A 17-chunk file (64 KiB chunks). For every chunk index i: flip one byte; swap with each other chunk (136 pairs); drop chunk i; duplicate chunk i. Whole-file cases: remove the last chunk; remove the last byte; remove the first chunk; append a valid chunk sealed with the same key and next counter; append 1 byte; clear the final flag on the last chunk; set it on chunk 0; insert a chunk of another file sealed with the same key.

**Expected result**: flip, swap, drop, duplicate, foreign chunk: `ChunkAuthFailed{index}` at the first differing index; removed last chunk or byte, cleared final flag: `Truncated` from `finish()`; appended chunk or byte: `TrailingData`. `finish()` never returns Ok on an altered stream; plaintext returned before the failing index equals the original (the application discards it, FT-N-13 step 3). **Max time**: 60 s.

### IT-19-4: Header tamper at every offset

**Summary**: Any altered header byte is refused; bounds fields give typed errors, others fail authentication. **Traces to**: AC-61 (FT-N-16, NFT-SEC-41 at crate level)

**Description**: A valid generation-1 header. (a) Replace each byte at every offset, one at a time, by three values (xor 0x01, xor 0xFF, 0x00), wrapped blobs and commitment included. (b) The DS-19 corpus, 10,000 seeded mutations. (c) Swap the passphrase-wrapped and recovery-wrapped blobs.

**Expected result**: accepted headers = 0 in all cases; malformed length: `HeaderMalformed`; version byte: `UnknownVersion`; authenticated region: `HeaderAuthFailed` or `WrongPassphraseOrTampered`; KDF fields: IT-19-5; error text carries no field value or secret. **Max time**: 600 s (the harness derives the KEK once and reuses it for mutations outside the KDF fields through the test-only KDF hook).

### IT-19-5: Argon2 parameter bounds, checked before any Argon2 work

**Summary**: Header-supplied KDF parameters are bounded before they can cost time or memory. **Traces to**: AC-61 (NFT-SEC-41 steps 2 to 5 at crate level)

**Description**: Craft structurally valid headers with forged fields and call `unlock` and `create_vault`; the test build exposes a call counter on the internal KDF function and a counting allocator. Rejected: t in {0, 1, 2, 11, 255, u32::MAX}; m in {63 MiB, 513 MiB, 64 GiB, u32::MAX KiB}; p in {0, 2, 4}; chunk size in {64 KiB - 1, 4 MiB + 1, 0, u32::MAX}; salt of 15 and 17 bytes; format version 0, current + 1, 0xFF. Accepted: t 3 and 10; m 64 MiB and 512 MiB (native); p 1; chunk 64 KiB and 4 MiB.

**Expected result**: each rejected case returns `ParamsOutOfBounds{field}` naming the field (version: `UnknownVersion`) with KDF call counter 0, peak allocation below 8 MiB and elapsed time below 50 ms; each accepted case runs exactly one derivation.

**Status**: READY for t, p, chunk size, salt, version and the native memory range. BLOCKED for the memory ceilings of WASM and mobile: the spec leaves them to the S7 benchmark (19 section 3); the cases are added when the numbers exist. **Max time**: 30 s.

### IT-19-6: Key commitment

**Summary**: A header whose wrapped key opens but belongs to another vault is refused. **Traces to**: AC-13, AC-61

**Description**: The oracle, which knows the passphrase, builds headers whose wrapped vault key authenticates under the real KEK but (a) unwraps to a key K2 while the commitment belongs to K1; (b) has a recovery wrap that unwraps to a key different from the passphrase wrap; (c) is a genuine header of vault V1 relabelled with the `vault_id` of V2 and re-authenticated.

**Expected result**: (a), (b), (c): `KeyCommitmentMismatch` from `unlock` and `unlock_with_recovery`, no `UnlockedVault`; the genuine header unlocks (positive control). **Max time**: 30 s.

### IT-19-7: A fresh wrapped file key per revision

**Summary**: Each revision has its own key; one revision's envelope cannot open another's ciphertext. **Traces to**: AC-60, AC-13

**Description**: One file id, 1,000 `begin_encrypt` calls of the same plaintext with real randomness. Open revision n's ciphertext with revision m's envelope for 1,000 random pairs (n != m). Serialise `FileEnvelope{header, wrapped_key}` to the JSON stored in `revisions.client_meta` and parse it back.

**Expected result**: distinct file keys and distinct nonce prefixes 1,000 of 1,000; cross-revision opens 1,000 of 1,000 rejected (`ChunkAuthFailed{index: 0}`), same-revision opens succeed; JSON round trip byte-equal and of constant length.

**Status**: READY. BLOCKED only for comparing the envelope length with the `client_meta` `CHECK`: the bound is not stated in `data_model.md`; the test records the length meanwhile. **Max time**: 60 s.

### IT-19-8: Header generation counter and rollback pin

**Summary**: A returning client never accepts an older header than it has seen. **Traces to**: AC-61

**Description**: Create (generation 1), `rewrap_header` twice (2 and 3). Unlock each header with `pinned_generation` None, 1, 2, 3. Edit the generation field of a generation-1 header to 3 and of a generation-3 header to 1. Call `rewrap_header` on a header at `u64::MAX`.

**Expected result**: header g with pin p: accepted when g >= p, `HeaderRolledBack{seen: g, pinned: p}` when g < p; pin None accepts any (the stated limit: a device that never saw the vault has nothing to pin); an edited generation field fails `HeaderAuthFailed` in both directions (the field is inside the authenticated header); `rewrap_header` at `u64::MAX` returns an error and never wraps to 0 (the variant is not yet named in `VaultError`); after a rewrap the old passphrase fails, the new one works and the recovery key still unlocks every generation. **Max time**: 30 s.

### IT-19-9: Passphrase policy and generated passphrases

**Summary**: Short or weak passphrases are refused; generated ones always pass. **Traces to**: RESTRICT-33, AC-14 (FT-N-37 at crate level)

**Description**: `check_passphrase` on 11 characters; 12 characters scoring zxcvbn 3; 12 or more scoring 4; 12 emoji (12 characters, 48 bytes); the empty string. `generate_passphrase` 10,000 times with a seeded RNG, and once with an RNG that returns the maximum value on every draw.

**Expected result**: 11 characters `TooShort`; score 3 `TooWeak`; score 4 Ok; length counted in characters; generated passphrases are 6 words from the 7,776-word list, 10,000 of 10,000 accepted by `check_passphrase`, no duplicates, word index always below 7,776, no modulo bias (chi-square over 777,600 draws, p > 0.001). If the web build uses `zxcvbn-ts` (19 risk 5) the same cases run against it and the verdicts match the native ones on all five listed passphrases. **Max time**: 60 s.

### IT-19-10: Recovery key

**Summary**: The recovery key unlocks the vault, resists typos and is never kept. **Traces to**: RESTRICT-22, AC-13

**Description**: `create_vault` returns the recovery key once. Unlock with it; mutate it (1,000 single-character substitutions, 1,000 adjacent transpositions, wrong group length, lower-case input); rewrap to a new passphrase and unlock with the recovery key again; search the header, all error values and `Debug` output for the key bytes.

**Expected result**: genuine key unlocks; substitutions and transpositions: `RecoveryKeyInvalid` in 2,000 of 2,000 (checksum); lower case behaves as one documented rule; after rewrap the recovery key still unlocks and the old passphrase does not; recovery-key bytes found anywhere outside the one returned value = 0. **Max time**: 30 s.

### IT-19-11: Swaps of wrapped keys and ciphertext

**Summary**: 1,000 randomised swaps are rejected. **Traces to**: AC-60 (FT-N-15, NFT-SEC-40 at crate level)

**Description**: 3 vaults with 5 files each. 1,000 seeded swaps from: envelope and ciphertext of file B opened as file A (same vault); the same across vaults; wrapped key only; ciphertext only; an envelope of vault V2 opened by V1's `UnlockedVault`; the format version byte set to 0, 2, 255.

**Expected result**: 1,000 of 1,000 rejected (`WrappedKeyRejected`, `ChunkAuthFailed{index: 0}` or `UnknownVersion`); the positive control opens.

**Status**: READY. Owner decision at Step 5: the AAD binds vault and file only (AC-60 reworded), so the parent-swap case is replaced by a positive case: the same file moved between folders of the vault still opens. **Max time**: 120 s.

### IT-19-12: Size limit and chunk-size contract

**Summary**: The encryptor refuses what the header cannot describe. **Traces to**: AC-14

**Description**: `begin_encrypt` with 100 MiB (ok) and 100 MiB + 1; `next_chunk` with a chunk 1 byte over the declared size, a short chunk before the end, and a total shorter or longer than the declared plaintext length.

**Expected result**: `TooLarge{limit: 104857600}` for 100 MiB + 1 (and success once `max_file_bytes` is 200 MiB); `ChunkSize` for an oversize or undersize non-final chunk; `LengthMismatch` from `finish()`; no ciphertext emitted after an error. **Max time**: 30 s.

## Performance Tests

### PT-19-1: Unlock time

**Summary**: Key derivation with the defaults fits the AC-14 budget. **Traces to**: AC-14

**Load scenario**: 20 sequential `unlock` calls with the defaults (64 MiB, t=3, p=1), one thread.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Native unlock p95 on the REF-SRV class CPU | at most 1 s | 1 s |
| Browser unlock p95 (system tier, NFT-PERF-29) | at most 2 s | 2 s |

**Resource limits**: peak memory at most Argon2 memory + 8 MiB. Phone and Safari values come from the S7 benchmark at system tier; a workstation run is `NOT-RUN(HW-REF)`, never a substitute.

### PT-19-2: Memory while encrypting and decrypting 100 MiB

**Summary**: The block-sized API keeps memory near one chunk. **Traces to**: AC-14

**Load scenario**: encrypt then decrypt a 100 MiB file in 4 MiB chunks through the incremental API, native, counting allocator.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Peak allocation above the unlocked-vault baseline | at most 3 chunks (12 MiB) | 16 MiB |

The 250 MB totals on iOS Safari and Android are NFT-RES-LIM-08.

## Security Tests

### ST-19-1: No secrets in Debug, errors, logs or panics

**Summary**: Key material never reaches text output. **Traces to**: AC-13, RESTRICT-23

**Attack vector**: log scraping and crash reports.

**Test procedure**: (1) `trybuild` compile-fail tests: `UnlockedVault` is not `Clone`; `Debug` prints no key bytes. (2) Run 1,000 failing calls from IT-19-3 to IT-19-11, capturing every `Display`, `Debug` and panic message. (3) Search the text for the passphrase, the recovery key, and 8-byte windows of the vault key and file keys.

**Pass criteria**: matches = 0 and `UnlockedVault: Clone` does not compile. **Fail criteria**: any match.

### ST-19-2: Zeroize on lock and drop

**Summary**: Key bytes are gone from process memory after lock. **Traces to**: AC-62

**Attack vector**: memory inspection after the user locks the vault.

**Test procedure**: (1) Unlock, decrypt a file, call `lock`; scan the test process memory (native: writable mappings of `/proc/self/mem`) for the vault key, the KEK and the file keys. (2) Repeat with `drop` and no `lock`. (3) WASM: terminate the module Worker on lock and scan the heap snapshot (system tier, NFT-SEC-42 step 3), because WASM linear memory cannot be guaranteed wiped (19 section 7).

**Pass criteria**: key byte matches after lock or drop = 0 on native. **Fail criteria**: any match.

### ST-19-3: Constant-time comparison of secrets

**Summary**: Commitment, tag and checksum checks do not use `==`. **Traces to**: AC-61, RESTRICT-23

**Attack vector**: timing of the commitment check.

**Test procedure**: CI source gate: every comparison of commitment, tag or checksum bytes goes through `subtle::ConstantTimeEq` (a `clippy::disallowed_methods` rule plus a grep gate over the crate); a deliberate `==` on a commitment is the positive control and must fail the gate.

**Pass criteria**: findings = 0 on the crate, 1 on the control. **Fail criteria**: any finding on the crate.

### ST-19-4: Randomness and nonces

**Summary**: Every nonce, salt and key comes from the injected source and none repeats. **Traces to**: RESTRICT-23

**Attack vector**: nonce reuse under one key.

**Test procedure**: (1) 10,000 wraps and 1,000 file encryptions with the OS source; collect the 24-byte nonces and 19-byte prefixes. (2) A `RandomSource` that fails: the operation returns `Rng` and emits nothing. (3) Grep gate: the WASM glue never calls `Math.random`; the crate calls no `getrandom` directly.

**Pass criteria**: duplicates = 0; output after an `Rng` error = 0 bytes. **Fail criteria**: any duplicate.

### ST-19-5: Fuzzing

**Summary**: Hostile headers and chunks cannot crash or exhaust the client. **Traces to**: AC-61, AC-13

**Attack vector**: malicious server data.

**Test procedure**: cargo-fuzz targets for the header parser and the chunk decryptor, seeded from DS-19 and IT-19-3. CI smoke 10 minutes per target, nightly 2 hours; the same corpus replays in V8 through the WASM build.

**Pass criteria**: panics, out-of-memory kills and inputs running over 1 s = 0 (typed errors only). **Fail criteria**: any crash.

## Acceptance Tests

### AT-19-1: Create, lock, unlock two ways, decrypt 100 MiB

**Summary**: The user path of the crate works with both secrets. **Traces to**: AC-13, AC-14

**Preconditions**:
- Native build; the 100 MiB file of DS-18.

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `generate_passphrase`, `create_vault`, keep the recovery key | Vault created; recovery key returned once |
| 2 | Encrypt the 100 MiB file; `lock` | Ciphertext differs from plaintext; keys zeroized |
| 3 | `unlock` with the passphrase, decrypt | Hash equals the original |
| 4 | `lock`; `unlock_with_recovery`, decrypt | Hash equals the original |

### AT-19-2: Release gate: external review

**Summary**: The construction is reviewed before v1.2 ships. **Traces to**: AC-62

**Preconditions**:
- Format frozen; vectors and fuzz results available to the reviewer.

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Hand over the specification, vector file, fuzz corpus and the open points of 19 sections 3 and 11 | Review report received |
| 2 | Record unresolved findings | 0 unresolved; report stored with the release (manual, `EXT`) |

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| vector file | known answers from the independent oracle | committed fixture from `tools/vault-oracle` | < 1 MB |
| vault-file (DS-18) | 100 MiB, 101 MiB, empty, 1, 2, 16 chunks at 64 KiB and 4 MiB | generated, seeded | about 210 MiB |
| vault-fuzz (DS-19) | 10,000 mutated headers | generated from a header written by `vault-header-gen` | about 2 MB |
| zxcvbn fixtures | the five passphrases of IT-19-9, scores checked against the reference implementation | fixture | < 1 KB |

**Setup**: `make test-data SET=vault` writes DS-18 and DS-19 into `tests/data-cache/`; the oracle regenerates the vector file only when the format version changes (a reviewed commit). **Teardown**: none (temporary directories only). **Isolation**: pure functions with injected randomness; each test builds its own vault; no shared mutable state.
