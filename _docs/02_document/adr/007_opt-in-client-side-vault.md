# ADR-007: Opt-in client-side encrypted vault (v1.2)

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Users want some folders the server operator cannot read. Everything else on the server needs plaintext access: the media pipeline makes thumbnails and transcodes (AC-10, AC-11), share links serve files to anonymous browsers (AC-12), and the API decrypts blobs for workers (ADR-006). End-to-end encryption removes the server's key, so every one of those features stops working for the encrypted data. Research on E2EE products shows that independent implementations of the same format break (ETH/CCS 2024, `security_clients.md` source 32 and findings 38 and 39), that Web Crypto has neither XChaCha20 nor Argon2, and that the one construction chosen, `aead-stream`, has no stated audit. The scope is therefore deliberately narrow and gated by an external review.

- Acceptance criteria addressed: AC-13 (server stores ciphertext only; tampered, truncated, reordered or wrong-vault data rejected), AC-14 (100 MiB files, unlock at most 1 s native and 2 s browser, camera backup never writes into the vault), AC-60 (1,000 of 1,000 key/ciphertext swaps rejected), AC-61 (header bounds and authentication, 0 of 10,000 mutations accepted), AC-62 (strict CSP, zeroize on lock, external review before release)
- Restrictions addressed: RESTRICT-03 (v1.2 only, after external review), RESTRICT-22 (separate passphrase, one-time recovery key, loss of both is unrecoverable), RESTRICT-23 (one shared Rust core, XChaCha20-Poly1305, Argon2id 64 to 512 MiB, at least 3 passes, parallelism 1, authenticated header, key commitment, no server thumbnails or transcoding), RESTRICT-33 (passphrase of at least 12 characters plus a strength check)
- Risks addressed: R23 (the external review may change the format; unaudited `aead-stream`), R35 (the key-wrap does not bind the parent folder; a malicious server can move, hide or roll back vault files)
- Research source: `_docs/01_solution/solution.md` § Component: Vault (v1.2)

## Decision

We will ship the vault in v1.2 as an opt-in, single-owner, end-to-end encrypted folder tree whose cryptography lives only in one client-side Rust core (`vault-core`), and release it only after an external review of that construction passes.

**Construction.** `vault-core` is compiled native, to WASM (module Worker, single thread) and through UniFFI, with a shared test-vector file as a CI gate. A passphrase (at least 12 characters and a zxcvbn check) is stretched with Argon2id (default m=64 MiB, t=3, p=1) into a key-encryption key that wraps a random 256-bit vault key; a separate 256-bit recovery key, shown once, wraps the same vault key. The header (format version, generation, KDF parameters, salt, both wrapped keys, key-commitment value) is authenticated by using the whole serialised header as AAD, and unlock rejects any parameter outside the bounds before running Argon2: t in 3..=10, m from 64 MiB to the platform cap, p=1, chunk size 64 KiB to 4 MiB, known `format_version` only. The header carries a generation counter; `rewrap_header` increments it, and clients pin the last generation seen per vault and refuse an older header (`HeaderRolledBack`).

**Files.** Each revision gets a fresh random file key, wrapped by the vault key with AAD = `vault_id || file_id || format_version` (not the parent, so moves, restores and conflict copies by clients that cannot re-wrap keys keep working). The wrapped key and file header travel in that revision's `revisions.client_meta`, so key and revision commit atomically through the normal commit path. Content is XChaCha20-Poly1305 STREAM with the plaintext length in the authenticated header, then follows the ordinary upload path, where the server encrypts the ciphertext again at rest (ADR-006). The server (`20_vault_service`) stores the opaque header in the `vaults` table with `ETag` and `If-Match` (412 on mismatch) and never links the crypto crate.

**Scope limits that make it opt-in.** Vault files get no thumbnails, transcodes or media jobs; camera backup never targets a vault; vault files cannot be link-shared in v1.2 (owner decision); files over 100 MiB (configurable) are refused; a vault has a single owner. Because server transcoding and previews need plaintext, only folders the user marks as a vault pay this cost.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| End-to-end encryption for everything (Ente or Filen style) | Would disable every server-side feature at once: thumbnails and transcodes (AC-10, AC-11), share links (AC-12), server-side hash checks. Nextcloud's folder E2EE shows the pattern: lost web access, previews and search, slow large files (`security_clients.md`, finding 40). Only a minority of self-hosted products ship E2EE, and those that do lose web, sharing and search (`b_ops_licence_scope.md`, scope table row "E2EE vault"). |
| No end-to-end encryption at all | Contradicts the problem statement ("folders marked as a vault are end-to-end encrypted") and AC-13, which requires a downloaded vault file to differ byte for byte from plaintext with the server holding no key. |
| Server-held vault keys (server encrypts per folder) | Not end-to-end: the operator and anyone with the master key or a database dump could read the files, which fails AC-13 and the point of RESTRICT-22. It is already the model for non-vault data (ADR-006). |
| AES-256-GCM chunks via Web Crypto, separate per-platform implementations | Web Crypto has AES-GCM but lacks Argon2 and is not streaming, so WASM or JS would be needed anyway for the KDF; independent per-platform implementations are the documented failure class (CCS 2024). One Rust core compiled three ways avoids three ports and one set of vectors covers all. Truncation and reorder protection would also be hand-built. |
| libsodium `secretstream` | Audited and widely implemented, but needs a C toolchain on WASM, Android and iOS and sequential state complicates resume. Kept as the documented fallback if the reviewer rejects `aead-stream`; the wire format then changes, which is why `format_version` exists from day one. |

## Consequences

### Positive

- The server, operator, database dump or blob volume yields only ciphertext; both passphrase and recovery key are needed offline, and tampering with any header byte, chunk, key wrap or file identity is detected client-side (AC-13, AC-60, AC-61).
- Hostile headers cannot make a client burn memory or time: out-of-bounds Argon2 parameters are rejected before any KDF run.
- Key and revision commit atomically, with no separate key call and no per-node key table; per-revision file keys keep restores of earlier revisions decryptable.
- The vault is opt-in and separate crates and tables: v1.0 and v1.1 do not depend on it, and a delayed review only delays v1.2.
- One core means identical behavior on web, Android and iOS, and the XChaCha20 STREAM construction is shared with blob encryption.

### Negative

- R35: because the parent is bound in neither the key wrap nor the chunk AAD, the folder tree, names, which revision is current and parentage are server-chosen and unauthenticated. A malicious server can move, hide or roll back vault files but cannot read or alter content. The generation counter protects only a returning client; a never-seen vault has nothing to pin. This departs from the wording of RESTRICT-23 ("associated data binds vault ID, file ID and parent") and must be accepted by the reviewer.
- R23: the external review may force a format change; `aead-stream` is unaudited and the libsodium fallback costs a C toolchain on all three targets. The schedule has slack for this by design.
- The header lives on the server, so a stolen database allows unlimited offline passphrase guessing at Argon2id(64 MiB, t=3) speed. The passphrase check, the generated six-word option and the recovery key reduce but do not remove this; the product text must state it.
- Sizes, file counts, tree shape and timing remain visible to the server. Losing passphrase and recovery key makes the data unrecoverable.
- No server previews, no link sharing and a 100 MiB cap make the vault a niche feature, and its Argon2 timing on low-end phones and Safari is still unmeasured (spike S7).

### Neutral / Open

- Whether file names are ciphertext, and one versus many vaults per account, are undecided (`data_model.md` A6, `20_vault_service` risk 1). This ADR does not decide them.
- The passphrase threshold is zxcvbn score 4 or a generated passphrase (restrictions, F17 and `19_vault_core` agree after Step 4); the external reviewer may tighten it before release.
- Vault link sharing beyond v1.2 would need its own key design and review.

## Evidence

- `_docs/02_document/components/19_vault_core/description.md` § 3. Format and construction (proposal for the review); § 11. Risks and open points
- `_docs/02_document/components/20_vault_service/description.md` § 1. High-Level Overview; § 5. Data Access Patterns; § 10. Risks and open points
- `_docs/02_document/data_model.md` § 5.2 `revisions` (`client_meta`); § 10.2 `vaults`
- `_docs/02_document/system-flows.md` § Flow F17: Vault [v1.2]
- `_docs/02_document/canonical-names.md` § Technical defaults applied at Step 4 (vault files cannot be link-shared); § Open owner items
- `_docs/02_document/risk_mitigations.md` § Risk Register (R23, R35); § Defects Fixed in the Evaluator Pass (Step 4a)
- `_docs/00_research/04_reasoning_chain.md` § Dimension 8: Vault
- `_docs/00_research/raw/security_clients.md` § 3. Vault cipher and format; § 4. KDF and key hierarchy
- `_docs/00_research/06_component_fit_matrix.md` § Top-level matrix (Vault AEAD, KDF, Web crypto glue rows)
- `_docs/00_problem/restrictions.md` § Encryption and vault [v1.2]; `_docs/02_document/tests/restriction-ids.md` (RESTRICT-03, RESTRICT-22, RESTRICT-23, RESTRICT-33)

## Notes

Release gate: nothing of the vault ships in v1.0 or v1.1, and v1.2 ships only after the external review passes with no unresolved findings (AC-62). Include R35 and the unaudited `aead-stream` explicitly in the review scope. `architecture.md` § 7 and § 8 still say "AAD binds vault, file and parent" and `system-flows.md` F17 steps 3 and 6 still mention the parent and a share link; `canonical-names.md` and the component specs win, and those lines should be corrected in the next doc pass. Re-evaluate the libsodium `secretstream` fallback if the review rejects `aead-stream`.
