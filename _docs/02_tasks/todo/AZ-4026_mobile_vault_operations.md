# Mobile vault operations: create, unlock, list, upload, download, lock [v1.2]

**Task**: AZ-4026_mobile_vault_operations
**Name**: Mobile vault operations
**Description**: The v1.2 vault calls the mobile app makes, joining the vault core with sync-core's transfer: create a vault (folder, header, recovery key), unlock with the passphrase or recovery key (one failure report on a wrong passphrase), list a vault folder, upload a file as chunked ciphertext with its wrapped file key in the same commit, download and decrypt in chunks with tampering rejected, and lock
**Complexity**: 5 points
**Dependencies**: AZ-4025_mobile_vault_core_library, AZ-3991_backup_core_surface, AZ-3960_sync_tree_operations, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3964_verified_download, AZ-4008_passphrase_policy, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4012_lock_zeroize, AZ-3913_vault_decisions_spike, AZ-3923_vault_header_service, AZ-3924_vault_unlock_failure_report, AZ-3926_vault_routes_registration, AZ-3927_vault_membership_wiring, AZ-4003_android_emulator_harness; notes: new task `N-05_mobile_vault_core_library` the parent resolves its key
**Component**: 17 android_app (`mobile_backup` and Kotlin layer) [v1.2]
**Tracker**: AZ-4026
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/n02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/20_vault_service/vault_http_api.md` — create, header read and replace, unlock-failure report (it names the 17/18 client tasks as consumers "not planned yet").
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — create, unlock, encryptor and decryptor, lock (UniFFI form).
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `create_folder`, `list_children`, `upload`, `stage_small`, `batch_commit`, `download`.
- `_docs/02_document/contracts/17_android_app/backup_core.md` — the surface this task extends (its "Not covered: v1.2 vault calls").

## Problem

F17 runs on "web/mobile/desktop" clients, and NFT-SEC-39 counts tampered variants accepted "on web, desktop and Android". The phone needs the same operations as the web vault page (AZ-3954): create, unlock, upload, download and lock, with every key on the device and content passed one block at a time so memory stays bounded (AC-14, 19 §4). The vault core gives the crypto (E-27/n01) and sync-core gives the upload and download protocol, but nothing joins them: `backup_core.md` excludes v1.2 vault calls and `sync_api.md` has no vault route. This task adds that join so the screen (E-27/n03) and later iOS have one path to call.

Split from the planned "mobile vault screen" row: the operations and the screen together are above 5 points. This task holds the operations; E-27/n03 holds the screen.

## Outcome

- Create: the passphrase is checked by the vault core's policy only; the core creates the vault key, recovery key and header; a vault root folder is created; the header is stored with `POST /api/v1/vaults`; the recovery key is returned once for display.
- Unlock: the header is read; the core unlocks with the passphrase or the recovery key; a wrong passphrase decrypts nothing and sends exactly one unlock-failure report with no body content.
- List: the children of a vault folder, with names as the server has them (name handling as fixed by E-30/c01).
- Upload: the file size is checked against the vault limit; content is encrypted chunk by chunk and uploaded as ciphertext through sync-core's upload paths; the wrapped file key travels in `client_meta` in the same commit as the revision (F17 step 4), with the membership marker E-30/c01 fixes.
- Download: ciphertext through sync-core's verified download; decryption chunk by chunk; tampered, truncated, reordered or wrong-vault data ends in a typed error, and no partial plaintext is left as a finished file.
- Lock: the vault handle is locked and dropped; later calls fail with a typed error.
- Every call is block-sized across the UniFFI boundary; no key, passphrase or recovery key is written to disk, logs, Dart or intents.

## Scope

### Included
- The operations above in the mobile library and Kotlin layer, their typed errors, and the extension of `backup_core.md` with a v1.2 section.
- Emulator tests against the real v1.2 server stack.

### Excluded
- The Flutter screen (E-27/n03); passphrase change and header replace with `If-Match` (not asked for mobile, see Flagged concerns); vault share links (none in v1.2, owner decision 2026-11-05); camera backup into a vault (never, AC-14).
- iOS host code (E-28).

## Acceptance Criteria

**AC-1: Create follows the core's policy (FT-N-37, AC-14)**
Given a signed-in account on the v1.2 stack and no vault
When create is called with an 11-character passphrase, a 12-character passphrase of zxcvbn score 3, a 12-character passphrase of score 4, and a generated passphrase
Then the first two are refused with the core's reasons and create 0 vaults and 0 folders, the last two each create one vault whose header the server stores unchanged, and the recovery key is returned once and is not stored anywhere on the device

**AC-2: Unlock and wrong passphrase (AC-14, AC-61, NFT-PERF-29 native)**
Given an existing vault
When it is unlocked 20 times with the right passphrase on the low-end Android phone, once with the recovery key, once with a wrong passphrase, and once with a header that has one altered byte
Then every right-passphrase unlock takes at most 1 s, the recovery key unlocks, the wrong passphrase decrypts nothing and sends exactly one unlock-failure report, and the altered header is refused

**AC-3: Ciphertext round trip (AC-13, FT-P-44 steps 1-3, FT-P-46, NFT-SEC-39 steps 1-2)**
Given an unlocked vault
When files of 100 MiB, 0 bytes, and exactly 1, 2 and N chunks are uploaded and downloaded
Then the bytes the server stores differ from each plaintext with 0 known-marker matches, each revision carries its wrapped file key in the commit that created it, and every decrypted download's hash equals the original

**AC-4: Size limit (AC-14, FT-P-45 steps 1-2)**
Given the vault limit at 100 MiB, then set to 50 MiB
When a 100 MiB file and then a 51 MiB file are uploaded
Then the first is accepted, and the second is refused with an error naming the 50 MiB limit and no ciphertext of it stored

**AC-5: Tampering is rejected (AC-13, AC-60, NFT-SEC-39 step 4, NFT-SEC-40)**
Given a test server acting as malicious: one flipped bit, a chunk removed, two chunks reordered, a chunk of another file inserted, and a wrapped key and ciphertext pair swapped with another file's
When each is downloaded
Then each ends in a typed error, no output file is reported finished, and no partial plaintext remains

**AC-6: Lock and memory (AC-14, NFT-RES-LIM-08 Android leg)**
Given an unlocked vault with a known test key and a 100 MiB vault file
When the file is downloaded and decrypted, the vault is locked, a heap dump of the app process is searched, and a call is made on the locked handle
Then the app's memory peak is recorded with `dumpsys meminfo` and stays within 250 MB, 0 matches of the key bytes or passphrase are found, and the call fails with a typed error

**AC-7: Nothing on disk (RESTRICT-22, RESTRICT-23)**
Given a session that created, unlocked, used and locked a vault
When the app's files, preferences, databases, the backup archive and the logs are searched for the passphrase, the recovery key and the key bytes
Then 0 occurrences are found

## Non-Functional Requirements

**Performance**
- Unlock at most 1 s native (AC-14); memory bounded by block-sized calls (19 §5).

**Compatibility**
- v1.2 builds only; v1.0 and v1.1 contain none of this code (E-27/n01 AC-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Policy call and error mapping | No second passphrase rule; reasons passed through |
| AC-3 | Commit request for a vault file | Wrapped key in `client_meta` of the same commit; marker set |
| AC-5 | Decryptor error mapping and output handling | Typed error; output file not finished |

## Blackbox Tests

Emulator against the real v1.2 stack (harness E-27/c19); the timing and memory legs on the low-end Android phone.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Four passphrases | FT-N-37 (Android leg) | 2 refused, 2 created | AC-14 |
| AC-2 | Existing vault; low-end Android phone | NFT-PERF-29 native half | <= 1 s every run; one failure report | AC-14, AC-61 |
| AC-3 | 100 MiB and edge-size files | FT-P-44 steps 1-3; FT-P-46; NFT-SEC-39 steps 1-2 | Ciphertext stored; hashes equal | AC-13, AC-60 |
| AC-4 | Limit 100 MiB then 50 MiB | FT-P-45 steps 1-2 | Boundary exact | AC-14 |
| AC-5 | Malicious server variants | NFT-SEC-39 step 4 (Android); NFT-SEC-40 steps 1-2 | 0 accepted | AC-13, AC-60 |
| AC-6 | 100 MiB file; known key | NFT-RES-LIM-08 (Android leg) | Peak recorded <= 250 MB; 0 key matches | AC-14 |
| AC-7 | Full session | Device file and log search | 0 occurrences | RESTRICT-22 |

## Runtime Completeness

- **Named capability**: the vault operations of F17 on Android, over the real vault core and the real sync-core transfer.
- **Production code that must exist**: create, unlock, list, upload, download and lock calling the real vault routes, the real upload and download paths, and `vault-core` through UniFFI.
- **Allowed external stubs**: the malicious-server variants of AC-5 come from a test proxy or the E-31 server double; nothing else.
- **Unacceptable substitutes**: Android Keystore or a Java crypto library in place of the vault core, a Kotlin or Dart copy of the passphrase rule, a whole-file buffer in memory, keys persisted between unlocks.

## Constraints

- Block-sized calls only; the native wrapper owns the vault handle (ADR-013, 19 §5).
- Upload through F4 and download through F6, the same paths as other files; no second transfer implementation.

## Risks & Mitigation

**Risk 1: Upload path for small vault files**
- *Risk*: files at or below 2 MiB go through the small batch path, whose commit may not carry `client_meta`.
- *Mitigation*: see the first flagged concern; AC-3 covers a file of exactly 1 chunk.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `sync_api.md` has no vault operation and it is not stated whether `upload` and `batch_commit` carry `client_meta`; the four vault routes and the wrapped key need either sync-core operations (E-23) or calls owned by `mobile_backup`. This task assumes the join lives in 17 and that sync-core passes `client_meta`; if not, an E-23 task is needed first | E-23 / E-27 owners | open | High |
| The `backup_core.md` v1.2 section (the call shapes the screen and iOS consume) must be written by this task at implement time; it is a shared surface for E-27/n03 and E-28 | E-27 owner | open | Medium |
| File browsing and download on Android are out of v1.0 by owner decision; v1.2 vault download needs both for vault folders, and where a decrypted file is saved on the phone is not stated | Owner | open | Medium |
| The 250 MB memory bound of AC-14 is stated for iOS Safari and Chrome on a 4 GB Android phone; NFT-RES-LIM-08 monitors the Android app with `dumpsys meminfo`. AC-6 applies the same bound to the app until the owner confirms | Owner | open | Low |
| Passphrase change (header replace with `If-Match`, 412) is not in the mobile list; it is left to the web page | Owner | open | Low |
| How the client learns the vault size limit before upload is not stated; AC-4 relies on the server's check at upload start (03) and its error | E-30 owner | open | Low |
| The low-end Android phone of NFT-PERF-29 is owner-held and not yet named | Owner | open | Low |
| Membership marker, name handling in vault folders and one or many vaults per account are decided by E-30/c01 (AZ-3913) | E-30/c01 | open | Medium |
