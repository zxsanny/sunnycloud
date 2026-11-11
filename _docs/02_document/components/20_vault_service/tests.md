# Test Specification — 20 Vault Service [v1.2]

Tests of the server side of the vault: the opaque header with `ETag` and `If-Match`, the vault root, limits, the deletion hook, and the vault refusals that cross other components (no derived files, no share links). System scenarios that already cover the same ground are referenced, not restated: FT-N-13 to FT-N-16, FT-N-37, FT-P-44 to FT-P-47, NFT-SEC-39 to NFT-SEC-43, NFT-SEC-13 (two-account matrix), NFT-SEC-07 step 7 (device token on vault endpoints), FT-P-19 and NFT-RES-30 (account deletion). IDs here: IT, ST, AT. No performance tests: the header is a few hundred bytes read and written by primary key and no AC sets a bound.

**Where it runs**: Rust integration tier with real Postgres 18 and the real API (testcontainers), and as Compose scenarios in the `core` profile (`C`, `e2e-consumer`), release 1.2 only. Cases that need client crypto use the real `vault-core` (19), native build; the server is never given a key. **Status**: READY = defined by the component spec and an AC; BLOCKED names the missing piece.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-13 | Server stores only ciphertext for vault files | IT-20-6, ST-20-2; FT-N-13, FT-P-44, NFT-SEC-39 | Covered |
| AC-14 | 100 MiB limit (configurable) with a clear message; camera backup stays out | IT-20-5; FT-N-14, FT-P-45, NFT-SEC-43 | Covered |
| AC-59 | Vault routes in the two-account matrix, 0 cross-account successes | IT-20-7, ST-20-1; NFT-SEC-13 | Covered |
| AC-61 | Header returned byte-identically; concurrent replacement detected; rollback handled | IT-20-1, -2, -3; FT-N-16, NFT-SEC-41 | Covered; header cap 1024 (IT-20-1) |
| AC-27 | Account deletion leaves no vault rows | IT-20-8; FT-P-19, NFT-RES-30 | Covered |
| RESTRICT-22, RESTRICT-23 | Server holds ciphertext and an opaque authenticated header only | IT-20-3, IT-20-6, ST-20-2 | Covered |
| Owner decision "vault files cannot be link-shared in v1.2" (`canonical-names.md`) | A share of a vault file is refused | IT-20-4 | Covered (no AC; the decision is the source) |

## Blackbox Tests

### IT-20-1: Header round trip is byte-identical and the ETag is its SHA-256

**Summary**: The server stores and returns the header without parsing or altering a byte. **Traces to**: AC-61

**Description**: `POST /api/v1/vaults` with a header of random bytes for a live root folder; `GET /api/v1/vaults/{id}`; `GET` again after a server restart. Headers: 1 byte, 512 bytes, a block holding all 256 byte values, and headers of exactly `max_vault_header_bytes` and one byte more.

**Expected result**: create 201; GET returns the same bytes (BLAKE3 equal) before and after restart; `ETag` = SHA-256 of the stored bytes and equal on both reads; the header of max + 1 bytes is refused (`HeaderTooLarge`, 4xx) and nothing is stored.

**Status**: READY: `max_vault_header_bytes` defaults to 1024 (`20_vault_service` section 6, decided 2026-11-11), so the max case is 1024 bytes and max + 1 is 1025 bytes. **Max time**: 30 s. **Dependencies**: API, Postgres.

### IT-20-2: `If-Match` rules and a 50-way replacement race

**Summary**: Two clients cannot silently overwrite each other's header. **Traces to**: AC-61

**Description**: Create a vault, read ETag E0. `PUT /api/v1/vaults/{id}/header` without `If-Match`; with a stale ETag; with the current ETag and header H1. Then 50 concurrent PUTs from ETag E1, each with its own header and `If-Match: E1`.

**Expected result**: missing `If-Match`: 428, stored header unchanged; stale ETag: 412, stored header byte-equal to before; current ETag: 200 and a new ETag equal to SHA-256 of H1; 50-way race: exactly 1 answers 200 and 49 answer 412, the stored header equals the winner's bytes, 5xx = 0. **Max time**: 30 s.

### IT-20-3: Generation counter, stale header and a returning client (with 19)

**Summary**: The server cannot make a client accept an older header, and a client with a stale ETag must re-read. **Traces to**: AC-61, RESTRICT-23

**Description**: Devices A and B share one vault and use the real `vault-core`. A creates the vault (generation 1); B unlocks and pins 1. A changes the passphrase (`rewrap_header`, generation 2) and replaces the header with `If-Match`; B unlocks with pin 2. Then `e2e-ctl` acts as a malicious or restored server and writes the stored generation-1 bytes back into `vaults.header`; B opens again. In parallel C, holding the old ETag, tries to replace the header.

**Expected result**: A's replace 200; B unlocks the generation-2 header; after the rollback B's unlock returns `HeaderRolledBack{seen: 1, pinned: 2}` and opens no file; C's replace answers 412 and after re-reading C sees the current header and merges. A device that never saw the vault accepts the generation-1 header: this is the stated limit (19 section 3), recorded as expected, not a defect.

**Dependencies**: `vault-core` native build; a named `e2e-ctl` action that overwrites `vaults.header` (the consumer has no database access; the action is added with this scenario). **Max time**: 60 s.

### IT-20-4: No link sharing of vault files

**Summary**: A share cannot be created for a vault file or for anything in a vault. **Traces to**: owner decision (canonical-names), AC-13

**Description**: Inside a vault upload one ciphertext file. `POST /api/v1/shares` for the vault file, for the vault's root folder, and for a normal file beside the vault (positive control).

**Expected result**: vault file 422 `target_not_shareable`; vault root folder 422 `target_not_shareable` (folders are not shareable in v1.0 either); normal file 201; `share_links` rows for the vault file 0 (ctl probe). **Dependencies**: 06, and the vault membership answer (`nodes.vault_id`, `20_vault_service` section 10). **Max time**: 30 s.

### IT-20-5: Vault files get no media jobs and the size limit applies to them only

**Summary**: Files under a vault root are never given thumbnails or transcodes, and `max_vault_file_bytes` holds. **Traces to**: AC-14, AC-13

**Description**: Commit ciphertext files declared as `image/jpeg` and `video/mp4` inside the vault (the server cannot tell they are ciphertext, so the vault flag must decide) and the same bytes outside it as a control. Commit vault files of 100 MiB and 100 MiB + 1 (DS-18); raise `max_vault_file_bytes` to 200 MiB and commit 101 MiB; commit 101 MiB outside the vault.

**Expected result**: jobs for vault files 0 (`admin media-stats` unchanged) and the thumbnail route answers 404, while the control outside the vault gets its job; 100 MiB accepted; 101 MiB refused at the upload start with a message naming the limit and at most 2 MiB sent; accepted at limit 200 MiB; 101 MiB outside the vault is not subject to the vault limit. The outcome is asserted, not the mechanism by which 02/03 learn the node is in a vault (Gaps). **Dependencies**: 02, 03, 08. **Max time**: 120 s.

### IT-20-6: A database dump of a vault account holds only ciphertext and opaque bytes

**Summary**: Nothing the server stores lets it read a vault file. **Traces to**: AC-13

**Description**: After IT-20-5, dump the database and read the blob volume as `e2e-ctl` does; search for the plaintext marker of the uploaded file, and for the passphrase and recovery key the client used.

**Expected result**: markers, passphrase and recovery key found in the dump, blob files, audit rows and logs = 0; `vaults.header` equals the client's header bytes; `revisions.client_meta` holds a wrapped key, not a plaintext key. **Dependencies**: `e2e-ctl` raw blob read and read-only probe. **Max time**: 60 s.

### IT-20-7: Vault root must be a live folder of the caller

**Summary**: A vault can only be created on an existing node the caller owns. **Traces to**: AC-59

**Description**: `POST /api/v1/vaults` with root = a file; a tombstoned folder; a random UUID; a folder of another account; a live folder (control); then a second vault on the same root.

**Expected result**: another account's folder, unknown id and tombstoned folder: 404; a file: 4xx (code per the 11 mapping); control 201; second vault on one root refused (`root_node_id` is unique).

**Status**: READY for the root checks. A second vault in one account is READY: one vault per account (owner, 2026-11-11), so the second create is `VaultExists`, and a create on a non-empty root folder is refused too. **Max time**: 30 s.

### IT-20-8: Account deletion removes vault rows and ciphertext

**Summary**: The erase hook leaves no vault data behind. **Traces to**: AC-27

**Description**: An account with a vault, a header and 10 vault files, and a second account with its own vault. Delete the first (`DELETE /api/v1/accounts/me`), wait for the job (at most 15 min; the test config shortens the batch interval), probe.

**Expected result**: `vaults` rows of the first account 0 (the hook reports `remaining = 0`); its ciphertext blob files 0; the second account's header and files unchanged (hash-set equal). **Dependencies**: 07 `DeletionPurger`, ctl probes. **Max time**: 120 s.

## Security Tests

### ST-20-1: Vault routes in the two-account matrix and scope rules

**Summary**: Account B reaches nothing of A's vault; device tokens cannot change vault state. **Traces to**: AC-59

**Attack vector**: IDOR on the vault id and header; a sync-only token used for a session-only operation.

**Test procedure**: (1) Add `POST /vaults`, `GET /vaults/{id}`, `PUT /vaults/{id}/header` and `POST /vaults/{id}/unlock-failures` to the generated matrix of NFT-SEC-13; B calls each with A's vault id, with and without an `If-Match` taken from A. (2) With A's device token call `POST /vaults` and `PUT /vaults/{id}/header` (`SessionOnly`) and `GET /vaults/{id}`. (3) A cross-origin PUT with A's cookie (NFT-SEC-09 rule).

**Pass criteria**: B gets 404 on every call; the device token gets 403 on the two `SessionOnly` routes; cross-origin PUT 403; CI fails when a vault operation is missing from the matrix. **Fail criteria**: any 2xx for B, or an operation absent from the matrix.

### ST-20-2: Failure reports and logs carry ids only

**Summary**: The unlock-failure report never stores secrets or attempt text. **Traces to**: AC-13, RESTRICT-22

**Attack vector**: secrets leaking into audit rows, logs or error bodies.

**Test procedure**: (1) The client reports 20 unlock failures through `POST /api/v1/vaults/{id}/unlock-failures` with an empty body. (2) Send the same route a body holding a passphrase-shaped string and a 64-byte key-shaped value, and a request for an unknown vault id. (3) Read the `vault_failure` audit rows and the API logs.

**Pass criteria**: audit rows hold account id, vault id and time only; secret-shaped strings found in rows or logs = 0; the unknown id answers 404 and writes nothing. **Fail criteria**: any stored or logged body content.

## Acceptance Tests

### AT-20-1: Two devices, one vault, a passphrase change and a restored backup

**Summary**: The server side supports the user path without ever holding a key. **Traces to**: AC-13, AC-61

**Preconditions**:
- Release 1.2 stack; two signed-in devices on one account; real `vault-core` on both.

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Device A creates a vault and uploads a file | 201; file listed; no job created |
| 2 | Device B unlocks and downloads the file | Hash equals the original |
| 3 | A changes the passphrase | Header replaced with `If-Match`; B's next unlock needs the new passphrase |
| 4 | Operator restores a database backup taken before step 3 | B's unlock returns the rolled-back-header error (IT-20-3) |
| 5 | Delete the account | No vault rows or ciphertext remain (IT-20-8) |

## Gaps

- **Vault membership** (decided 2026-11-11, AZ-3913): membership is the nullable `nodes.vault_id` copied from the parent at creation (`20_vault_service` section 10). IT-20-4 and IT-20-5 assert outcomes; add these cases when E-30/c07 wires it: a file three levels deep under the vault root is a member (no media job, share refused, `max_vault_file_bytes` applied); a move of a file into a vault, out of a vault, or between two vaults is 409 `vault_boundary` and changes nothing; a move between folders of the same vault succeeds and keeps `vault_id`; a plaintext file with an active share link cannot be moved into a vault (409) and the vault create on a non-empty root is refused.
- **Header cap**: resolved 2026-11-11, default 1024 bytes (IT-20-1). **Vaults per account**: resolved 2026-11-11, one (IT-20-7). **Vault names**: ciphertext (owner, 2026-11-11); NFT-SEC-43 step 5 stands.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| vault-file (DS-18) | 100 MiB, 101 MiB, small ciphertext files | generated, seeded | about 210 MiB |
| headers | random bytes, a 256-value block, generations 1 and 2 from the real `vault-core` | generated at test start | < 1 MB |
| accounts | A, B, C and a second vault account | invite tokens created by `e2e-ctl` | n/a |

**Setup**: v1.2 test image with `max_vault_file_bytes` 100 MiB; accounts by invite; vault root folders created through the API. **Teardown**: account purge by `e2e-ctl`, except IT-20-8, which uses the deletion API on purpose. **Isolation**: every test has its own accounts and vault root; the rollback case of IT-20-3 and the config change of IT-20-5 run in group X (exclusive, stack reset first).
