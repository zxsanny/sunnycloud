# Vault pipeline [v1.2] (F17)

**Task**: AZ-3675_pipeline_vault
**Name**: Vault pipeline
**Description**: A client creates, unlocks and uses an end-to-end encrypted vault with the vault core, while the server stores only the header and ciphertext through the vault service and the normal upload and download paths, end to end against the real components
**Complexity**: 5 points
**Dependencies**: AZ-4009_vault_header_format, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4012_lock_zeroize, AZ-4013_vault_wasm_adapter, AZ-3913_vault_decisions_spike, AZ-3923_vault_header_service, AZ-3924_vault_unlock_failure_report, AZ-3925_vault_erase_hook, AZ-3926_vault_routes_registration, AZ-3927_vault_membership_wiring, AZ-3928_vault_integration_suite, AZ-3841_job_planner_enqueue_effect, AZ-3697_share_create_list_revoke, AZ-3948_vault_placeholder, AZ-3655_pipeline_large_upload, AZ-3657_pipeline_download, AZ-3665_pipeline_account_deletion, AZ-3954_web_vault_page, AZ-3652_initial_structure, AZ-4025_mobile_vault_core_library, AZ-4026_mobile_vault_operations, AZ-4027_mobile_vault_screen
**Component**: 20 vault_service (server side), driven by the client vault page (16)
**Tracker**: AZ-3675
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/p01

## Problem

F17 crosses the client crypto core (19, WASM in the web app), the vault service (20), the upload and download paths (03, 02, 04), the media planner (08 must skip vault files), sharing (06 must refuse them) and account erase (20's hook). E-30/c06 and E-30/c07 wire the server half and E-30/c08 tests it; no planned task drives the flow from a client through the server and back. The client side has no planned caller at all: E-26 scopes the vault page out and plans only a placeholder (E-26/c18).

## Outcome

- A vault created and unlocked on a client stores only ciphertext and wrapped keys on the server, uploads and downloads through the normal paths, gets no media jobs and no share links, and rejects tampered or swapped data on the client.
- Every component operation of F17 has a production caller, including the client calls of the vault core.
- At least one end-to-end test runs F17 against the real server stack.

## Scope

### Included
- Flow order and seams (`system-flows.md` F17 steps 1-7):
  1. Client → vault core `create_vault` (passphrase policy, Argon2id within bounds) → `VaultService::create_vault(header, root node)` through 11; recovery key shown once.
  2. Unlock → `VaultService::get_header` → vault core `unlock` (header bounds and authentication before any Argon2 work).
  3. Upload → vault core `begin_encrypt` (per-file key, AAD = vault id, file id, format version) → ciphertext through F4 with the commit context marking the vault (100 MiB limit) → `revisions.client_meta` holds the wrapped file key in the same commit.
  4. 08's planner creates no job for a vault file; 06 refuses a share whose target is a vault file.
  5. Download through F6 → vault core `begin_decrypt` rejects tampered, truncated, reordered or wrong-vault data.
  6. Passphrase change → vault core `rewrap_header` → `VaultService::replace_header` with `If-Match`; wrong passphrase → `report_unlock_failure` (audit ids only).
  7. Lock zeroizes keys; account erase removes the vault rows through 20's hook (E-22/p12 list).
- The end-to-end test of the steps above.

### Excluded
- Vault share links (not in v1.2), camera backup into a vault (never), the external review (E-29/c01), the vault page UI itself (not planned; see Flagged concerns).

## Acceptance Criteria

AC-1..AC-6 are local to this task. No interaction risk of `interaction-risks.md` lies on F17 (vault attacks are AC-60, AC-61).

### Wiring and flow criteria

**AC-1: Create, unlock, upload, download**
Given a signed-in client with the vault page and the real server
When the user creates a vault, unlocks it, uploads a 10 MiB file into it and downloads it
Then the decrypted download equals the original, the bytes the server serves for the file differ from the plaintext, and the revision's `client_meta` holds the wrapped file key

**AC-2: No media, no shares**
Given a JPEG uploaded into the vault
When the commit completes and the owner tries to create a share link for it
Then no job row exists for it and the share is refused with a typed error

**AC-3: Tampering is rejected on the client**
Given two files in the vault
When their ciphertexts are swapped on the server and one header byte is altered in a copy of the header
Then the client rejects both swapped downloads and refuses to unlock with the altered header

**AC-4: Limits and concurrency**
Given a 101 MiB file and a header changed by another client since this client read it
When the file is uploaded into the vault, and the header is replaced with the stale ETag
Then the upload is refused with a clear message, and the replace gets 412

**AC-5: Failure reports and erase**
Given a wrong passphrase, and later the account's deletion
When the client reports the failure and the account purge runs
Then the audit log has a `vault_failure` event with ids only, and the vault rows are gone after the purge

**AC-6: Every flow operation has a production caller**
Given the source tree (server and web)
When the wiring check searches non-test code
Then it finds production callers of `VaultService::{create_vault, get_header, replace_header, report_unlock_failure}` (vault routes), 20's hook in the list built by 12, the commit context's vault flag set on the commit path, and the vault core's `create_vault`, `unlock`, `begin_encrypt`, `begin_decrypt`, `rewrap_header` and `lock` called from the client vault page

## Non-Functional Requirements

**Performance**
- Unlock within 1 s native and 2 s in the browser; 100 MiB opens within 250 MB on iOS Safari (AC-14); measured in E-29/c14 and E-31/c49.

**Compatibility**
- The vault page runs under a CSP without `unsafe-inline` and `unsafe-eval` and loads no third-party host (AC-62).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over server and web sources | Each named call has a production caller |

## Blackbox Tests

Playwright against the real stack (v1.2 build); the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed-in client, vault page | Create, unlock, upload, download | Plaintext equal; server bytes differ; wrapped key stored | AC-13 |
| AC-2 | JPEG in vault | Commit; create share | No job; share refused | AC-14 |
| AC-3 | Two vault files | Swap ciphertexts; alter header | Both rejected; unlock refused | AC-60, AC-61 |
| AC-4 | 101 MiB file; stale ETag | Upload; replace header | Refused; 412 | |
| AC-5 | Wrong passphrase; account deletion | Report; purge | Audit ids only; vault rows gone | |

## Constraints

- Owner: component 20 (`src/components/20_vault_service/`) for this flow's server-side composition checks and the end-to-end test; the client calls live in the vault page, which needs its own E-26 task.
- Nothing of this task ships in v1.0 or v1.1 (ADR-016); the test runs only on v1.2 builds.
- Downstream: E-31/c49 ([v1.2] vault scenarios) should depend on this task.

## Risks & Mitigation

**Risk 1: Vault membership not known to the commit path**
- *Risk*: 02 and 03 cannot tell that a node is under a vault root, so media jobs or shares slip through.
- *Mitigation*: E-30/c01 decides the membership mechanism; AC-2 checks both refusals.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No planned task builds the client vault page (E-26 scope-out; only the placeholder E-26/c18) or a mobile vault UI, so the vault core has no production caller; the correct fix is an E-26 v1.2 task for the vault page, not a test-only client | Owner / E-26 | open | High |
| Vault membership mechanism and header cap are still a spike (E-30/c01) | E-30 | open | Medium |
| `wasm-unsafe-eval` in the web CSP must be re-checked against AC-62 before the vault page (E-26 risk 3) | E-26 | open | Medium |
