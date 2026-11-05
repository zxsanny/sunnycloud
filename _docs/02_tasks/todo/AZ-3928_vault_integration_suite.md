# Vault service integration, security and acceptance suite (IT-20-1..8, ST-20-1..2, AT-20-1)

**Task**: AZ-3928_vault_integration_suite
**Name**: Vault service test suite
**Description**: Run the component test specification of 20 against the real API and PostgreSQL 18 (testcontainers and the Compose `core` profile, v1.2 builds only), with the real `vault-core` native build for the client crypto cases, and hold the v1.2 release of 20 behind the external review
**Complexity**: 5 points
**Dependencies**: AZ-3922_vaults_migration_config, AZ-3923_vault_header_service, AZ-3924_vault_unlock_failure_report, AZ-3925_vault_erase_hook, AZ-3926_vault_routes_registration, AZ-3927_vault_membership_wiring, AZ-4009_vault_header_format, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4018_vault_acceptance_review_package, AZ-3820_deletion_purger, AZ-3697_share_create_list_revoke, AZ-3841_job_planner_enqueue_effect, AZ-3738_transfer_commit_upload, AZ-3755_transfer_download, AZ-3834_csrf_idor_matrices, AZ-3742_test_harness_skeleton, AZ-3652_initial_structure
**Component**: 20 vault_service [v1.2]
**Tracker**: AZ-3928
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/20_vault_service/vault_http_api.md` — the routes under test (E-30/c06).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — matrix generation and auth classes (E-21/c11, c18).

## Problem

Each E-30 task tests its own operation. The component test specification (`components/20_vault_service/tests.md`) also needs the cases that cross components and processes: a restart between write and read, a 50-way race over HTTP, a rollback written behind the service's back with a real client, a database and blob-volume dump, the account purge, the two-account matrix and the full two-device user path. The client-driven end-to-end flow F17 is `AZ-3675_pipeline_vault` (E-30/p01); this task stays at the server component boundary and uses `vault-core` only to produce real headers and ciphertext.

## Outcome

- IT-20-1 to IT-20-8, ST-20-1, ST-20-2 and AT-20-1 run green on v1.2 builds, in CI (testcontainers) and in the Compose `core` profile.
- The BLOCKED parts of IT-20-1 and IT-20-7 run with the E-30/c01 answers.
- The v1.2 release of 20 is gated on the signed external review report.

## Scope

### Included
- The ten test IDs above with their data sets (DS-18 vault files, generated headers, accounts by invite).
- A test-only `e2e-ctl` action that overwrites `vaults.header` with earlier bytes (IT-20-3 names it as added with this scenario), built in the E-31 harness with its owner.
- The coverage gate: 90% on the replace precondition and account-scoping paths.
- The release-gate check of AC-62 for 20 (manual step of NFT-SEC-42 step 6).

### Excluded
- The client vault page and the end-to-end flow (AZ-3675, and the unplanned E-26 vault page).
- System scenarios FT-P-44..47, FT-N-13..16, NFT-SEC-39..43 (E-31).
- Client-side header bounds and fuzzing (E-29).

## Acceptance Criteria

**AC-1: Header integrity over HTTP (AC-61, IT-20-1, IT-20-2)**
Given a live root folder and headers of 1 byte, 512 bytes, all 256 byte values, exactly the cap and the cap + 1
When each is created, read, the server restarts and each is read again, and then the `If-Match` cases and a 50-way PUT race run
Then reads are byte-identical (BLAKE3-equal) with an equal ETag derived from SHA-256, cap + 1 is refused with nothing stored, missing `If-Match` gives 428 and a stale one 412 with the header unchanged, and the race gives exactly 1 x 200, 49 x 412 and 0 x 5xx with the winner's bytes stored

**AC-2: Rolled-back header and stale ETag (AC-61, RESTRICT-23, IT-20-3)**
Given devices A and B with the real `vault-core`, A at generation 2 after a passphrase change, B pinned to generation 2, and C holding the old ETag
When `e2e-ctl` writes the generation-1 bytes back into `vaults.header`, B unlocks, and C replaces the header
Then B's unlock returns `HeaderRolledBack{seen: 1, pinned: 2}` and opens no file, C gets 412 and after re-reading sees the current header; a device that never saw the vault accepting generation 1 is recorded as the stated limit, not a defect

**AC-3: No shares, no jobs, the size limit (AC-13, AC-14, IT-20-4, IT-20-5)**
Given a vault with ciphertext files declared as JPEG and MP4 and the same bytes outside it
When shares are created and the commits complete, and 100 MiB, 100 MiB + 1 and (with the limit at 200 MiB) 101 MiB files are uploaded into the vault
Then vault targets get 422 `target_not_shareable` and 0 jobs (thumbnail route 404, `admin media-stats` unchanged) while the outside control gets its job, and the size results match IT-20-5

**AC-4: The dump holds only ciphertext (AC-13, IT-20-6)**
Given the data of AC-3 and the passphrase and recovery key the client used
When the database is dumped and the blob volume is read as `e2e-ctl` does
Then the plaintext marker, passphrase and recovery key are found 0 times in the dump, blob files, audit rows and logs; `vaults.header` equals the client's bytes; `revisions.client_meta` holds a wrapped key only

**AC-5: Root rules and vaults per account (AC-59, IT-20-7)**
Given a file, a tombstoned folder, a random UUID, a folder of another account and a live folder
When each is used as a vault root, then a second vault is created on the same root, then (per E-30/c01) a second vault on another root
Then the foreign, unknown and tombstoned roots get 404, the file a 4xx, the live folder 201, the second vault on one root is refused, and the per-account case follows the recorded decision

**AC-6: Account deletion (AC-27, IT-20-8)**
Given account 1 with a vault, a header and 10 vault files, and account 2 with its own vault
When account 1 is deleted through the API and the purge finishes (at most 15 min; test config shortens the batch interval)
Then account 1 has 0 `vaults` rows and 0 vault ciphertext blob files, 20's hook reports `remaining = 0`, and account 2's header and files are hash-set equal to before

**AC-7: Two-account matrix and scope rules (AC-59, ST-20-1)**
Given the generated matrix of NFT-SEC-13 with the four vault operations
When account B calls each with A's vault id (with and without A's `If-Match`), A's device token calls the `SessionOnly` routes, and a cross-origin PUT carries A's cookie
Then B gets 404 on every call, the device token 403, the cross-origin PUT 403, and CI fails when a vault operation is missing from the matrix

**AC-8: Failure reports and logs carry ids only (AC-13, RESTRICT-22, ST-20-2)**
Given a vault of A
When 20 failure reports with an empty body, one with a passphrase-shaped string and a 64-byte key-shaped value, and one for an unknown vault id are sent
Then audit rows hold account id, vault id and time only, secret-shaped strings are found 0 times in rows and logs, and the unknown id gets 404 and writes nothing

**AC-9: Two devices, one vault, a passphrase change and a restored backup (AT-20-1)**
Given a v1.2 stack and two signed-in devices on one account with the real `vault-core`
When the AT-20-1 steps run (create and upload, unlock and download on B, passphrase change on A, database backup restored by the operator, account deletion)
Then each step gives the AT-20-1 expected result, and no step gives the server a key

**AC-10: Release gate (AC-62)**
Given a v1.2 release candidate
When the release checklist runs without a signed external review report (evidence package of E-29/c15)
Then 20 is not released; with the report present the gate passes (NFT-SEC-42 step 6, manual)

## Non-Functional Requirements

**Reliability**
- Coverage: at least 75% of 20's business logic and 90% of the replace precondition and account-scoping paths; below 90% blocks (`quality-thresholds.md`).
- The rollback case of IT-20-3 and the config change of IT-20-5 run in group X (exclusive, stack reset first).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None new: unit tests live with E-30/c03..c07 | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real API + PostgreSQL 18; generated headers | Round trip with restart; `If-Match`; 50-way race | Byte-identical; 428/412; 1 x 200, 49 x 412 | IT-20-1, IT-20-2 |
| AC-2 | `vault-core` native on A, B, C; `e2e-ctl` overwrite | Rollback and stale ETag | `HeaderRolledBack{1, 2}`; 412 then re-read | IT-20-3 |
| AC-3 | DS-18; JPEG/MP4 ciphertext in and out of the vault | Shares, jobs, size limit | 422; 0 jobs; limit as IT-20-5 | IT-20-4, IT-20-5 |
| AC-4 | After AC-3 | Dump and blob volume search | 0 secret or plaintext matches | IT-20-6 |
| AC-5 | Five root kinds; second vault | Create vaults | 404 x3, 4xx, 201; second refused | IT-20-7 |
| AC-6 | Two accounts with vaults | Delete account 1; probe | 0 rows, 0 blobs; account 2 unchanged | IT-20-8, FT-P-19, NFT-RES-30 |
| AC-7 | Accounts A, B; device token | Matrix, scope, cross-origin | 404 / 403 / 403; CI fails on gap | ST-20-1, NFT-SEC-13 |
| AC-8 | Vault of A | 22 reports | Ids only; 404 for unknown | ST-20-2 |
| AC-9 | v1.2 stack, two devices | AT-20-1 steps 1-5 | As AT-20-1 | AT-20-1 |
| AC-10 | v1.2 release candidate | Release checklist with and without review report | Blocked without; passes with | NFT-SEC-42 step 6 |

## Constraints

- Runs only on v1.2 builds; tagged and skipped on earlier builds (E-31 rule).
- The server is never given a key; client crypto uses the real `vault-core` native build, never a stand-in.
- The consumer side has no database access; database writes behind the service go through named `e2e-ctl` actions only.

## Risks & Mitigation

**Risk 1: The account purge is slow in CI**
- *Risk*: the 15-minute budget makes IT-20-8 slow.
- *Mitigation*: the test config shortens the batch interval, as IT-20-8 allows.

## Runtime Completeness

- Named capability: the vault service behind the real API, with headers and ciphertext from the real vault core.
- Production code that must exist: 20's service, routes and hook; 02/03/06 membership wiring; `vault-core` native build.
- Allowed external stubs: none; `e2e-ctl` acts only as the malicious or restored server in IT-20-3 and AT-20-1 step 4.
- Unacceptable substitutes: random bytes in place of a real `vault-core` header in IT-20-3 and AT-20-1, an in-memory database, a test-only membership flag.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `e2e-ctl` overwrite action lives in `tests/`, owned by E-31; this task adds it with that owner | E-31 / E-30 owners | open | Low |
| `components/20_vault_service/tests.md` traces AC-61 to the server round trip and race, while AC-61's text in `acceptance_criteria.md` is the client's header bounds and tamper check (E-29); the server cases support AC-61 but do not prove it alone | Doc owner / E-29 | open | Low |
| `tests/traceability-matrix.md` still states AC-60 with "parent folder" in the list of rejected moves; the reworded AC-60 says a move to another folder of the same vault stays decryptable | Doc owner | open | Low |
