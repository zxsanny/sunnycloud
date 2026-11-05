# Web vault page [v1.2]

**Task**: AZ-3954_web_vault_page
**Name**: Web vault page
**Description**: The v1.2 vault page: create a vault under the passphrase rules, unlock with the passphrase or recovery key, upload and download vault files with the vault core's WASM in a module Worker (chunked encrypt and decrypt), change the passphrase, and lock by ending the Worker so no key stays in memory
**Complexity**: 5 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3939_transfer_service_small_path, AZ-3940_worker_large_upload, AZ-3942_download_strategies, AZ-3948_vault_placeholder, AZ-3950_playwright_web_harness, AZ-4008_passphrase_policy, AZ-4010_vault_create_unlock, AZ-4011_file_encryption_stream, AZ-4012_lock_zeroize, AZ-4013_vault_wasm_adapter, AZ-3913_vault_decisions_spike, AZ-3923_vault_header_service, AZ-3924_vault_unlock_failure_report, AZ-3926_vault_routes_registration, AZ-3927_vault_membership_wiring
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3954
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/n01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/20_vault_service/vault_http_api.md` — vault create, header read and replace with `If-Match`, unlock-failure report, produced by E-30/c06.
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — uploads and downloads (needs the v1.1.0 vault context this task adds), produced by E-26/c09.
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/components/19_vault_core/description.md` — vault-core operations and WASM rules (no contract file yet; E-29/c10 produces the WASM adapter).

## Problem

Owner decision 2026-11-04: the vault (F17, ADR-007) needs a web client in v1.2. Without it the vault core has no production caller on the web and the vault pipeline (`AZ-3675_pipeline_vault`, flagged High there) cannot run end to end. The page must keep every key on the client, never let the server or the browser storage see a key or passphrase, and stay within the browser memory and time limits of AC-14 and the strict CSP of AC-62. This task replaces nothing: the v1.0 placeholder (E-26/c18) stays for v1.0 and v1.1 builds.

## Outcome

- In a v1.2 build, `/vault` lets a signed-in user create, unlock, use and lock a vault; all crypto runs in the vault core's WASM inside a module Worker.
- Vault files upload and download through the normal transfer paths as ciphertext, with the vault marker and the wrapped file key in the same commit.
- Lock ends the Worker; a heap scan finds no key; storage holds no key or passphrase.
- The client calls of `create_vault`, `unlock`, `begin_encrypt`, `begin_decrypt`, `rewrap_header` and `lock` exist (AZ-3675 AC-6).

## Scope

### Included
- Create: passphrase entry checked by the vault core's policy (at least 12 characters and zxcvbn score 4, or a generated 6-word passphrase, FT-N-37), the generator, the recovery key shown once, then the vault created on the server with its header and root folder.
- Unlock with passphrase or recovery key after reading the header; a wrong passphrase reports the failure once (ids only) and decrypts nothing.
- Upload into the vault: per-file encryption in chunks in the Worker, ciphertext through `TransferService` with the vault marker and wrapped file key as fixed by E-30/c01 and E-30/c07; files above the vault limit (100 MiB, configurable) refused before upload with a clear message.
- Download from the vault: ciphertext through the download path, chunked decryption in the Worker, plaintext saved; tampered, truncated, reordered or wrong-vault data rejected and never saved as good.
- Passphrase change: `rewrap_header`, then header replace with `If-Match`; 412 shows "changed elsewhere" and re-reads.
- Lock: zeroise and end the Worker; the page returns to the locked state.
- The page under a CSP with no `unsafe-inline` or `unsafe-eval`, `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`, and no third-party host.

### Excluded
- Vault share links (none in v1.2, owner decision 2026-11-05); camera backup into a vault (never); mobile vault UI (E-27, E-28); the vault core and server (E-29, E-30).

## Acceptance Criteria

**AC-1: Passphrase rules at creation (AC-14, FT-N-37)**
Given a signed-in account without a vault
When the user tries an 11-character passphrase, a 12-character passphrase with zxcvbn score 3, a 12-character passphrase with score 4, and a generated passphrase
Then the first is refused with a message naming the 12-character minimum, the second is refused by the strength check, the last two create a vault (2 of 2), the refused attempts create 0 vaults, and the recovery key is shown once and appears nowhere after the panel closes

**AC-2: Unlock and wrong passphrase (AC-14, NFT-PERF-29)**
Given an existing vault
When it is unlocked with the right passphrase 20 times, once with the recovery key, and once with a wrong passphrase
Then unlock completes within 2 s at p95 in the browser, the recovery key unlocks it, and the wrong passphrase decrypts nothing and sends exactly one unlock-failure report

**AC-3: Ciphertext upload (AC-13, FT-P-44 steps 1-3)**
Given an unlocked vault
When a 10 MiB file is uploaded into it and then downloaded
Then the bytes the server serves differ from the plaintext, the revision carries the wrapped file key in the same commit, and the decrypted download's hash equals the original

**AC-4: Size limit (AC-14, FT-P-45)**
Given the vault limit of 100 MiB
When a 100 MiB file and a 101 MiB file are uploaded into the vault
Then the first is accepted, and the second is refused before any upload request with a message naming the limit

**AC-5: Tampering is rejected (AC-13, AC-60, AC-61)**
Given two vault files whose ciphertexts were swapped on the server, a truncated ciphertext and a header with one altered byte
When each is downloaded or the vault is unlocked
Then every download ends with a clear error and no saved file is reported as good, and the unlock with the altered header is refused

**AC-6: Passphrase change with a stale header**
Given another client changed the header after this page read it
When the user changes the passphrase
Then the replace answers 412, the page says the vault changed elsewhere and re-reads the header, and the stored header is the other client's

**AC-7: Lock leaves no key (AC-62, FT-P-47 step 3, NFT-SEC-42 step 3)**
Given an unlocked vault with a test key of known bytes
When the user locks it and a heap snapshot of the page and of every Worker and WASM memory is searched
Then the Worker has ended, 0 matches of the key bytes or the passphrase are found, and the page shows the locked state

**AC-8: CSP and hosts (AC-62, FT-P-47 steps 1-2, NFT-SEC-42 steps 1-2)**
Given the vault page served by the v1.2 stack
When its CSP is parsed and unlock plus a file open run with network logging
Then the CSP has no `unsafe-inline` and no `unsafe-eval`, has `object-src 'none'`, `base-uri 'none'` and `frame-ancestors 'none'`, and 100 % of requests go to the own origin

**AC-9: Nothing in browser storage (NFT-SEC-42 step 4)**
Given a session that created, unlocked, used and locked a vault
When `localStorage`, `sessionStorage`, IndexedDB, Cache Storage and cookies are inspected
Then none holds a vault key, the recovery key or the passphrase

**AC-10: Memory for a 100 MiB file (AC-14, NFT-RES-LIM-08)**
Given a 100 MiB vault file
When it is opened and decrypted on iOS Safari (iPhone 12 or newer) and Chrome on a 4 GB Android phone
Then total memory peaks at no more than 250 MB on each

**AC-11: No share links for vault files (canonical-names, AZ-3675 AC-2)**
Given a vault file
When its row menu opens, and create-share is called for it through the API
Then the page offers no share link, with the reason shown, and the API answers `TargetNotShareable`

## Non-Functional Requirements

**Performance**
- Unlock at most 2 s in the browser (AC-14, NFT-PERF-29); 100 MiB open within 250 MB (NFT-RES-LIM-08).

**Compatibility**
- v1.2 builds only, behind the E-26/c18 build flag; v1.0 and v1.1 builds contain none of this code (FT-P-73).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Page passes the passphrase to the vault core's check and shows its reason | No second rule in the page; messages per reason |
| AC-4 | Size check before upload | Refused above the limit; no request |
| AC-6 | 412 handling on header replace | Re-read; no local overwrite |

## Blackbox Tests

Playwright against the v1.2 stack (harness E-26/c19a); phones per environment.md.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Four passphrases | FT-N-37 | 2 refused, 2 created | AC-14 |
| AC-2 | Existing vault | NFT-PERF-29 (browser half) | p95 <= 2 s; one failure report | AC-14 |
| AC-3, AC-4 | 10 MiB, 100 MiB, 101 MiB files | FT-P-44 steps 1-3; FT-P-45 | Ciphertext stored; limit enforced | AC-13, AC-14 |
| AC-5 | Swapped, truncated, altered data | NFT-SEC-39, NFT-SEC-40 (web half) | Rejected; nothing saved as good | AC-60, AC-61 |
| AC-7..AC-9 | Known test key | FT-P-47; NFT-SEC-42 steps 1-4 | 0 key matches; strict CSP; 0 third parties | AC-62 |
| AC-10 | 100 MiB file; iPhone and Android phone | NFT-RES-LIM-08 | <= 250 MB | AC-14 |
| AC-11 | Vault file | Share attempt | No share action; `TargetNotShareable` | AC-14 |
| — | — | NFT-SEC-42 step 5 | Removed by owner decision 2026-11-05: no vault share links in v1.2 | — |

## Runtime Completeness

- **Named capability**: the vault core (19) compiled to WASM, run in a module Worker: passphrase policy, `create_vault`, `unlock`, `unlock_with_recovery`, `begin_encrypt`, `begin_decrypt`, `rewrap_header`, `lock`.
- **Production code that must exist**: the page and Worker calling those operations over the real vault routes and the real upload and download paths.
- **Allowed external stubs**: none.
- **Unacceptable substitutes**: Web Crypto or a JavaScript crypto library in place of the vault core, a page-side copy of the passphrase rule, keys kept in the page context, a test-only client.

## Constraints

- No vault code in v1.0 or v1.1 builds (ADR-016, E-26/c18).
- Randomness only from `crypto.getRandomValues`, passed to the vault core (19 §1).
- WASM memory cannot be guaranteed wiped, so lock ends the Worker (19 §6).

## Risks & Mitigation

**Risk 1: `wasm-unsafe-eval` under AC-62**
- *Risk*: the vault page needs `'wasm-unsafe-eval'` to run WASM, which the AC-62 review may refuse (epic risk 3).
- *Mitigation*: E-26/c01 records per engine whether it is needed; the reviewer decides before release.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Owner brief 2026-11-04, 16 §5 and NFT-SEC-42 step 5 say vault share links carry the key in the URL fragment; canonical-names ("Vault files cannot be link-shared in v1.2", which wins), 19 §1 and risk 7 (no link-key operation), 06 and `AZ-3675_pipeline_vault` (AC-2: share refused) say there are no vault links in v1.2. This task follows canonical-names (AC-11); if the owner confirms fragment-key links for v1.2, 19 needs a link-key design and this task an AC for NFT-SEC-42 step 5 | Owner | resolved | High |
| Resolved by owner decision 2026-11-05: no vault share links in v1.2; AC-11 stands, and NFT-SEC-42 step 5, 16 §5 and F17 were corrected | Owner | resolved | High |
| 16 §5 says the passphrase rule is zxcvbn score 3; FT-N-37, restrictions, 19 and ADR-007 say score 4 or generated (G-23 decision). AC-1 follows FT-N-37; 16 §5 needs the fix | E-26 owner (doc) | open | Low |
| The vault membership mechanism and commit marker are decided by E-30/c01 and wired by E-30/c07; `transfer_service.md` needs its v1.1.0 vault context from that decision | E-30/c01, E-26 | open | Medium |
| One or many vaults per account is undecided (E-30/c01); the page layout depends on it | Owner | open | Medium |
| No vault screens are drawn (DESIGN §8 "Not drawn: vault page"); screens must be drawn and approved before implementation | Owner (design) | open | Medium |
| The vault page CSP adds `object-src 'none'`, and the header is deployment-owned with no planned emitter (see E-26/c06) | Deployment owner | open | Medium |
| If zxcvbn is too large for the WASM build, 19 risk 5 moves it to a JavaScript library and the rule exists twice | E-29 owner | open | Low |
| A client-side limit on unlock attempts is an open question in 19 (open question 10) | E-29 owner | open | Low |
