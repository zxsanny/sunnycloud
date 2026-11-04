# Helper: IDs and hashing

**Crate**: `sc_ids` (also built for `wasm32-unknown-unknown`, hashing part only). **Used by**: 01_identity (tokens), 02_tree, 03_transfer, 04_blobstore (block verification), 06_sharing (tokens), 12_operator_cli, 13_sync_core, 14_desktop_daemon (via 13), 17_android_app (`mobile_backup`), 16_web_app (WASM hasher), 19_vault_core.

## Purpose
Every identifier, content hash and token rule in one crate, so server and clients agree byte for byte on block hashes, file hashes and commit IDs, and so token comparison is constant time everywhere.

## Interface

| Operation | Input | Output | Errors |
|-----------|-------|--------|--------|
| `new_id` | `&dyn Clock` | `Uuid` v7 (time-ordered, random tail) | none |
| `hash_block` | `&[u8]` (one block, at most 2 MiB) | `Hash32` (BLAKE3) | none |
| `FileHasher` | `update(&[u8])`, `finish_block()`, `finish()` | ordered `Vec<Hash32>` block list and `FileHash` | `HashError::BlockTooLarge` |
| `rollup` | `&[Hash32]` | `FileHash` | none |
| `derive_client_key` | `device_id: Uuid`, `asset_id: &str`, `FileHash` | `Uuid` (stable for the same inputs) | none |
| `new_token` | `kind: TokenKind` | `RawToken` (256-bit CSPRNG, URL-safe base64, never logged) | `RngUnavailable` |
| `new_user_code` | none | `UserCode` (8 characters base-20, RFC 8628 5.1) | `RngUnavailable` |
| `sha256_token` | `&RawToken` or `&str` | `TokenHash` (SHA-256, 32 bytes) | none |
| `ct_eq` | `&[u8]`, `&[u8]` | `bool` (constant time, equal-length check inside) | none |

## Rules fixed here
- **Block** = fixed 2 MiB slice (the last may be shorter). **Block hash** = BLAKE3 of the block bytes.
- **File hash** (`revisions.content_hash`) = BLAKE3 in `derive_key` mode with the context string `sunnycloud file v1`, over the ordered concatenation of the 32-byte block hashes. A zero-length file is the rollup of an empty list. The rule matches data model section on `content_hash` ("whole-file hash from the ordered block hashes"); the server rolls up without re-reading file bytes (F4 step 5). Clients verify downloads block by block and compare the rollup.
- **Commit ID for mobile** = first 16 bytes of keyed BLAKE3 (key from the context `sunnycloud commit v1`) over `device_id || asset_id || file_hash`, with UUID version and variant bits set. Desktop and web use random UUIDv7 commit IDs.
- **Tokens**: 256 random bits for sessions, device tokens, share tokens, invites; 128-bit minimum is the AC floor, 256 is the choice. Only `sha256_token` output is stored. Lookups are by `UNIQUE` hash column; where a token is compared in memory, `ct_eq` is used.
- Short user codes (device grant) use `new_user_code`; a `user_code` is shown to a person and is not a bearer secret, so it is stored **plain** (data model 4.5), while the `device_code` is a `new_token` value stored hashed. The mobile one-time login code (`login_codes`) is a `new_token` value stored hashed.
- `new_id` takes a `Clock`, so ids in tests are deterministic with `ManualClock`.

## Tests
FT-P-05 (block conformance), FT-P-38 and NFT-SEC-16 (random, hashed share tokens), AC-76/AC-75 unit tests (single-use), FT-N-01/02 (bit flip detected), AC-46 and FT-P-32 (commit ID stable across reinstall), cross-implementation vector test: native, WASM and Kotlin/Swift-via-UniFFI produce identical `FileHash` for a fixed corpus (blocks of 0, 1, 2 MiB, 2 MiB + 1 byte).

## Risks
- Rollup rule drift between server and clients silently breaks "server has hash" checks: the shared vector file is a CI gate for every consumer.
- BLAKE3 in WASM speed is browser dependent; the hasher exposes block-sized calls only so the Worker keeps its heap bounded (AC-50).
