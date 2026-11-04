# ADR-006: Application-level envelope encryption of every blob

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The problem statement says non-vault data is "encrypted at rest on the server", but no mechanism was named. Research flagged it as an unowned requirement (`b_ops_licence_scope.md` F6, `b_security.md` F28) and gave two options: operator-provided volume encryption (LUKS or ZFS native, the recommended and simplest option, which keeps `sendfile`, Range reads and worker input simple) or application-level envelope encryption with a key kept outside the data volume. The owner chose application-level encryption anyway. The decision is non-trivial because blobs are the hot path: AC-2 needs about 112 MB/s per transfer on unknown operator hardware, Range reads must stay cheap, media workers must receive plaintext without holding keys, and scrub must verify bytes it can only read as ciphertext.

- Acceptance criteria addressed: AC-43 (per-blob keys wrapped by an operator master key, rewrap without rewriting blobs, 4 KiB range read at most 5 ms p95), AC-2 (at least 90% of link speed with encryption on), AC-72 (1 GiB internal fetch equals plaintext hash; refuse to start on a key mismatch), AC-24 and AC-80 (scrub), AC-38 (acknowledge only after flush), AC-65 (backup and restore with the master key)
- Restrictions addressed: RESTRICT-08 (XChaCha20-Poly1305 STREAM in 64 KiB chunks, AES-GCM not used, master key never stored with the data), RESTRICT-32 (thumbnails and transcodes encrypted like every other blob), RESTRICT-13 (workers hold no master key and no blob mount)
- Risks addressed: R08 (hand-built random-access STREAM nonces; possible nonce reuse), R22 (one master key for all tenants; `rewrap` does not change data keys)
- Research source: `_docs/01_solution/solution.md` § Component: Storage, metadata and encryption at rest (row "Application-level blob encryption")

## Decision

We will encrypt every blob (user files, thumbnails, transcodes) in the API process with XChaCha20-Poly1305 in the STREAM layout (`chacha20poly1305 =0.11.0` with `aead-stream =0.6.0`), 64 KiB plaintext chunks, a fresh random 256-bit data key per blob wrapped by an operator-supplied master key, and a key check that makes a wrong key fail closed at start.

**Format.** A blob file is a fixed 64-byte header (magic, format version, STREAM nonce prefix, padding) followed by chunks of 64 KiB plus a 16-byte tag. Chunk `i` lives at `64 + i * (65536 + 16)`, so each of the 32 chunks of a 2 MiB block is written at a deterministic offset from any upload lane and a plaintext Range maps to a chunk range without an index. The nonce is prefix + 32-bit chunk counter + last-chunk flag; the declared size is known at session start so out-of-order writers know which chunk is last. AAD is the `blob_id`. Because the sequential `aead-stream` encryptor cannot seek, chunks are sealed with the raw cipher and a hand-built STREAM-format nonce; `aead-stream` stays as the test oracle and each known-answer vector (chunk 0, 1, n-1, single-chunk, empty file, refusal at the counter limit) is compared byte for byte.

**Keys.** The data key is wrapped with XChaCha20-Poly1305 under the master key of id `master_key_id`, AAD = `blob_id`, and stored in `blobs.wrapped_key`; there is no key cache (one AEAD call per request). The master key is a Docker secret or file mounted in the API container only. `<blob_root>/.keycheck` holds a sealed constant per key id, created at first start only when the `blobs` table and the volume are both empty; a missing file with blobs present is `KeyCheckAbsentButBlobsExist` and the operator runs `sunnycloud admin keycheck-init`, which first unwraps a sample of existing keys. Rotation is a key ring with one current id for new blobs; `sunnycloud admin rewrap --new-key-file` re-wraps data keys in batches until 0 rows remain, with no blob rewrite.

**Data path.** The API decrypts for downloads and for the internal worker endpoint, so neither the supervisor nor the decoder ever holds the master key or a blob mount. Scrub runs inside the server binary and verifies every chunk tag. Volume encryption stays recommended on top, because application encryption protects a stolen disk or backup, not a running or malicious server.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Volume encryption only (LUKS, ZFS native, fscrypt) | The research recommendation, kept as a layer on top but not as the mechanism. It does not meet AC-43 as written (per-blob keys, rewrap without rewriting data) and protects nothing once a blob is copied off the encrypted device, for example into a file backup. The owner explicitly chose per-blob keys. fscrypt also leaves sizes and mtimes visible (`storage_db.md` F28). |
| Per-user keys | Share links (AC-12) and media workers act without the owner's session, so the server would need those keys anyway; per-user keys add key-management and recovery work without protecting against a server that decrypts on request. Not benchmarked or separately researched. Open question 8 in `security_clients.md` asked for the choice; the owner chose one install-wide master key. |
| AES-256-GCM chunks | Software AES-GCM measures about 0.12 GB/s on a fast core without AES-NI/PCLMUL, roughly the 0.112 GB/s AC-2 needs and below it on slower or VM-masked CPUs; ChaCha degrades to 0.5 to 0.95 GB/s. With AES hardware it is faster (2.04 vs 1.16 GB/s) but that hardware precondition on unknown operator hosts is the deciding cost (`mve_round2.md` decision logic). A version-2 header with AES-GCM is the planned upgrade path if crypto CPU share exceeds 30% on the AC-1 test. |
| `age` file format per blob | Not covered by the research cards; assessed at ADR time. Its payload is the same STREAM idea but it carries per-recipient headers and offers no random-access API, so the Range requirement (4 KiB read at most 5 ms, measured 57 µs at 64 KiB chunks) would still need a hand-built reader. A master-key wrap through our own `wrapped_key` column is simpler. |

## Consequences

### Positive

- A stolen disk, volume copy or file backup without the master key yields no plaintext (AC-43); the key never lies on the blob volume and `.keycheck` stops a wrong key at start (AC-72).
- Measured cost is small: decrypt 1.16 GB/s and encrypt plus write 0.85 GB/s on one core against 0.112 GB/s required, about 11% of a core for a 125 MB/s aggregate; a 4 KiB random read takes 57 µs.
- Rotation is cheap and online: `rewrap` touches only `blobs.wrapped_key`, never the data.
- Decoders and the supervisor are structurally key-free, which supports AC-25 and RESTRICT-13; derived blobs are covered by the same path.
- A single construction (XChaCha20 STREAM) is shared with the vault core (ADR-007), so one set of test vectors serves both.

### Negative

- Forecloses kernel `sendfile` and proxy file offload: stored bytes are ciphertext, so every download is a userspace read, decrypt and write. AC-2 does not depend on it, but no sendfile comparison was measured.
- Loss of the master key makes all data unrecoverable; backups must hold the key separately, with the rotation rule that old keys stay while retained backups need them.
- R22: one key for all tenants, and `rewrap` does not change data keys, so a leaked old key plus an old dump still decrypts those blobs. Accepted for v1.0; a data-key re-encrypt tool is a post-v1.0 option.
- R08: hand-built random-access nonces are unreviewed. The rule "verify the block hash before `write_block`, so only hash-consistent bytes are ever written at a given nonce" mitigates it; a client that changes a block between two attempts is the residual case. An external review of the blob encryption before v1.0 is an open owner item.
- Workers need a decrypted copy: the API streams plaintext to the supervisor over the plaintext internal listener, which is why `:8081` is internal-network-only.

### Neutral / Open

- Master key provisioning and backup follow `environment_strategy.md` (two independent copies, never with the data backups); this ADR does not change that rule.
- If the reviewer rejects the hand-built construction, the contingency is a reviewed STREAM implementation per 64 KiB chunk with a format-version bump and the rewrap tool.
- Blob counter limit is 2^32 chunks (about 256 TiB per blob); a 50 GiB file is 819,200 chunks.

## Evidence

- `_docs/02_document/components/04_blobstore/description.md` § 4. File Format and Encryption; § 8. Security Notes; § 12. Risks
- `_docs/02_document/architecture.md` § 2. Technology Stack (Blob encryption row); § 7. Security Architecture (Data protection)
- `_docs/02_document/components/12_operator_cli/description.md` § 3. Command surface (`admin rewrap`, `admin keycheck-init`)
- `_docs/02_document/deployment/environment_strategy.md` § Master-key backup rule (AC-43, AC-65); § Per-environment differences (volume encryption on top)
- `_docs/02_document/deployment/deployment_procedures.md` § Backup and restore (AC-24, AC-30, AC-41, AC-43, AC-65)
- `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — RustCrypto chacha20poly1305 0.11.0 `XChaCha20Poly1305` + aead-stream 0.6.0 (benchmarks and decision logic)
- `_docs/00_research/raw/b_security.md` F28 and `_docs/00_research/raw/b_ops_licence_scope.md` F6 (disk-level option recommended); `_docs/00_research/06_component_fit_matrix.md` § Round 2 changes (owner decision)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R08, R22)
- `_docs/00_problem/restrictions.md` § Technology; `_docs/02_document/tests/restriction-ids.md` (RESTRICT-08, RESTRICT-32)

## Notes

This is an owner decision against the written research recommendation (option A, volume encryption, "simplest, cannot be silently weakened"). `architecture.md` § 8 lists it as "ADR-005" in its short index; the file number here follows the Step 4.5 plan. Header size in the older MVE card (offset `9 + ...`) is superseded by the 64-byte header in the blobstore spec. Re-evaluate AES-GCM (header version 2) when a 10 GbE install shows crypto above 30% of serving cores, and decide the external-review scope and date before the v1.0 release.
