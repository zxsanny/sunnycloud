# 16_web_app

## 1. High-Level Overview

**Purpose**: The browser client: sign in, browse and manage files, upload and download large files with bounded memory, create share links, approve desktop devices, and run the small admin section.

**Single responsibility**: the user interface and the browser-side transfer engine. It owns no server logic and stores no tokens: the session is an HttpOnly cookie managed by the server.

**Architectural pattern**: Svelte 5 SPA (runes, Vite build, static assets), typed API client generated from the OpenAPI document, a module Web Worker for transfers, one `TransferService` facade for the pages.

**Upstream dependencies**: 11_http_api (public API under `/api/v1`, OpenAPI-generated client); 13_sync_core WASM build (`sync-core-wasm`: BLAKE3 block hashing, file-hash rollup, commit ID, upload resume planning; see Boundary below); helpers 05 (via that WASM), 06 (optional name preflight via `sc_names` WASM, otherwise server errors only); 19_vault_core WASM (v1.2).
**Downstream consumers**: end users; Caddy serves the built assets.

### Scope
In: sign-in page and setup-token first-run page, file browser (paged), upload (single request, batch, sliced large), download, share dialogs (files only in v1.0) and the public share page, device approval page, device and session list (revoke), account page, admin section (users, invites, quotas, health), thumbnail display and polling, inline-vs-attachment rules, CSP requirements, vault page placeholder (v1.2).
Out: any API or storage logic, any token handling in JS, server-side rendering, offline mode, sharing between accounts other than links, version history and trash (out of product scope), folder sync (desktop's job).

**Boundary with 13 (confirmed by 13)**: the block protocol (2 MiB blocks, 4 lanes, resume, commit rollup) is implemented once in Rust by 13. The browser cannot run its HTTP client, so the Worker drives `fetch` itself while calling the *sans-IO* WASM module for every rule that must match the server byte for byte: block hashing, rollup, commit ID, resume diff (`need_blocks`). Without this module the web app would carry a third protocol implementation, which this design rejects.

## 2. Internal Interfaces

### Interface: TransferService (page-facing, Svelte stores)

| Method | Input | Output | Async | Error Types |
|--------|-------|--------|-------|-------------|
| `enqueueUpload` | `File[]`, target folder id | `UploadJobId[]` | Yes | `InvalidName`, `QuotaExceeded` (HTTP 507), `SizeTooLarge`, `SessionCapReached`, `FreeSpaceFloor`, `ParentNotFound`, `AccountNotActive`, `TooManyFiles` |
| `resumeUpload` | `File` (re-selected), `UploadJobId` | `Resumed{resent_bytes}` or `Restarted{reason: ChangedSinceLastTime}` | Yes | `FileUnreadable`, `UploadSessionGone` |
| `cancelUpload` | `UploadJobId` | `()` | Yes | none |
| `download` | node id or share token, optional revision | `DownloadOutcome::{VerifiedSaved, SavedUnverified(reason), Failed(reason)}` | Yes | `HashMismatch`, `Forbidden`, `NotFound`, `Network` |
| `progress` | none | store of per-job `{bytes_done, state, lanes}` | No | none |

### Worker protocol (page to module Worker, structured clone, no `File` held by the page after hand-off)
`start{file, parent, name, baseRevision?}`, `resume{file, session}`, `cancel`, replies `progress`, `done{revision}`, `failed{typedError}`.

### Interface: ApiClient (generated; thin wrapper adds `X-Request-Id`, the header `X-Sunnycloud-Client: web/<version>`, and typed `ApiError`)
Operations used: server info (`GET /api/v1/server-info`), auth (session info, sign-out, providers list, setup), tree (list page by `name_key` cursor, create folder, rename, move, delete), uploads (create session, get session, put block, commit, `PUT /api/v1/staged-uploads/{staged_key}`, batch commit), download/range, changes (not used by the page; browser does not sync), shares (create, list, revoke, public open, password unlock), devices (list, revoke, approval lookup/approve/deny), admin (users, invites, quotas, health), thumbnails (state per listing item).

### Interface: ContentDisposition rules (pure function `renderMode(mediaType) -> 'inline' | 'attachment'`)
Inline only: `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `video/mp4`, `video/webm`. Everything else (HTML, SVG, XML, JS, PDF, polyglots) is offered as a download link, never put in an `<img>`, `<video>`, `<iframe>`, `<object>` or `srcdoc`. The server enforces the same list with `nosniff` and a sandbox CSP (AC-47); the page rule is defence in depth and the media type it uses is the server's, never the file extension.

## 2a. Ports

Same format as 13. The browser is not a Rust component, so its boundaries are the wire API and the WASM exports.

| Direction | Port | Defined in | Counterpart |
|---|---|---|---|
| Requires | public HTTP API, generated typed client | OpenAPI document of 11 | 11_http_api |
| Requires | block hashing, rollup, commit ID, resume diff | `sync-core-wasm` (wasm-bindgen API) | 13_sync_core |
| Requires (v1.2) | vault crypto | 19 WASM build | 19_vault_core |
| Provides | none (static assets served by Caddy) | | |

## 3. External API
None served. Static assets only: `index.html`, hashed JS/CSS, WASM modules, no inline scripts.

## 4. Data Access Patterns

| Data | Where | Lifetime |
|------|-------|----------|
| Session | HttpOnly, Secure, SameSite cookie (server) | server-controlled |
| Upload resume record: `{fileName, size, lastModified, sessionId, blocklist}` | IndexedDB, per user | until commit or session expiry (24 h rolling) |
| UI preferences (view mode, sort) | `localStorage`, wrapped in try/catch, optional | per browser |
| Listings | in memory, paged (keyset cursor), virtualised list | per page view |

No token, password or share password is ever written to browser storage. A 100k-file selection creates 100k `File` objects: the queue caps in-flight objects (`web.max_inflight_files`, default 200).

## 5. Implementation Details

**Upload engine (AC-50, AC-3, AC-4)**: files up to `small_file_max` (2 MiB) are staged with `PUT /api/v1/staged-uploads/{staged_key}` (the key is the file's `client_key`) and committed in one `POST /api/v1/commits:batch` of at most `batch_max_entries` entries with a random UUIDv7 commit ID. The batch (commit ID and staged keys) is kept in memory for the life of the tab and reused on every retry, so a retried batch gets the stored response; a closed tab abandons it and the staged rows expire. `StagedCapReached` means: commit what is staged, then stage more. Each large upload uses a random UUIDv7 `client_key` stored with its IndexedDB resume record and reused on every resume. A `409 BlockConflict` is passed to the WASM planner (retry after a session re-read, or restart). `NothingToSend` ends an upload with 0 bytes. Larger files: the Worker reads 2 MiB with `Blob.slice`, hashes each block with the WASM hasher, builds the blocklist, creates the session, then sends blocks with plain `fetch(PUT)` on up to 4 lanes (one block per request; no streaming request bodies). Heap stays at most 150 MB (worker + page). Before commit it re-reads `size` and `lastModified`; any change aborts and restarts (FT-N-03). Resume after tab close: the user re-selects the file; if name, size and `lastModified` equal the IndexedDB record, the Worker reads the session bitmap and sends only missing blocks (at most 10 MiB re-sent); otherwise it restarts. The saved blocklist avoids re-hashing 50 GiB on resume; the server still verifies every block hash on PUT, so a stale blocklist cannot corrupt data. A phone locking or the tab closing ends the upload; the page says so and keeps the resume record. Folder upload is files only (`webkitdirectory`; empty folders only by drag and drop); large trees point to the desktop client.

**Downloads (AC-7, AC-51)**: strategy chosen per browser and size, outcome always labelled.
1. Chromium with `showSaveFilePicker`: stream to disk, hash incrementally block by block, verified at any size.
2. Firefox and Safari up to 2 GiB: verified path (mechanism to be fixed by spike S12: streamed fetch with incremental hash and disk-backed save; no whole-file `Blob` in heap); outcome `VerifiedSaved`.
3. Above 2 GiB on Firefox and Safari: native `<a download>` of the API URL; the page states "not verified in the browser" and shows the server-provided file hash for manual check. Heap at most 200 MB in all three browsers.
Mismatch never ends as a saved "good" file (`HashMismatch`, partial file discarded where the API allows).

**Share dialogs**: files only (no share control on folders in v1.0); create (expiry default from server config shown in the field and capped by the server's maximum share lifetime, both read from the API; optional password), the URL is shown once with a copy button, list with revoke. Public page `/s/<token>`: calls the public API; missing/expired/revoked all render the same "link not available" page; password prompt on `password_required`; inline preview per `renderMode`, otherwise a download button; 429 shows retry-after. The page does not distinguish the failure reasons because the server does not.

**Device approval page** (F3, AC-57): lookup by user code, shows device name and platform, requester IP/location next to the approver's, a prominent warning on mismatch, expiry countdown, approve/deny. Only a cookie session can approve.

**Thumbnails**: listings carry a thumbnail state (`pending|ready|failed|none`); while the tab is visible the page polls pending items every `web.thumb_poll_interval` (2s), stops on `visibilitychange`, and shows a placeholder icon for `failed`.

**Admin section**: visible only when the session profile has the admin role; routes also guarded server-side. Users (list, deactivate/delete; removing or demoting the last admin is refused by the server and shown as such), invites (create single-use with expiry, revoke), quotas (default and per user), health (readiness, storage free, queue depth, worker count from the health endpoint). No billing, no organisations.

**CSRF and cookies**: all state-changing calls are `fetch` with same-origin credentials; the browser supplies `Origin` and `Sec-Fetch-Site`, which the server requires (AC-58). The page adds no token header and never reads the cookie.

**CSP requirement (edge config owned by deployment)**: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'` (WASM only; vault page v1.2 must serve without `unsafe-inline`/`unsafe-eval` and review whether `wasm-unsafe-eval` is acceptable under AC-62), `style-src 'self'`, `img-src 'self' blob:` where needed, `connect-src 'self'`, `frame-ancestors 'none'`, `base-uri 'none'`, `form-action 'self'`, no third-party hosts, no inline handlers. Build fails if the bundle contains an inline script or a non-self URL.

**Vault page (v1.2, placeholder in v1.0)**: route `/vault` exists behind a build flag and renders "not available in this version"; no vault code is shipped in v1.0 (FT-P-73). In v1.2 it uses 19_vault_core WASM in a module Worker with chunked decrypt, passphrase rules (12 characters, zxcvbn 3), locks by zeroing keys, share links put the key in the URL fragment.

**Key dependencies**: Svelte 5, Vite, TypeScript, generated OpenAPI client (openapi-typescript or equivalent), `sync_core_wasm` (wasm-bindgen, `--target web`, single-threaded; Vite integration needs a spike), Vitest + Playwright for tests.

**Error handling**: every API call returns `Result<T, ApiError>`; pages switch on `ApiError.code` (stable codes from 02_helper_error). 401 sends the user to sign-in; 426 `upgrade_required` shows the upgrade message; 429 shows retry-after; 507 `quota_exceeded` shows the quota message; unexpected errors show the correlation id.

## 6. Helpers
`sc_ids` and `sc_names` via WASM; shared corpus `names_corpus.json` and hash vectors for contract tests.

## 7. Caveats and Edge Cases
- Safari/iOS suspends background tabs: uploads pause; resume by re-select.
- `File` handles cannot be restored after reload (no File System Access persistence relied on).
- WASM BLAKE3 speed varies by browser; hashing must not run on the main thread.
- Large listings: keyset paging and a virtual list; no "select all" across pages without a server-side operation.
- Safari ITP and cookie rules apply; same-site deployment on one host avoids third-party cookie issues.

## 8. Dependency Graph
Must be implemented after: 11 OpenAPI document, 13 WASM module. Parallel with: 14, 15, 17. Blocks: v1.2 vault page.

## 9. Logging
Client-side errors go to the console in dev; in production only a minimal "report" payload with correlation id is offered to the user. No tokens, passwords, file names in telemetry; there is no third-party analytics.

## 10. Configuration
Build-time constants: API base (`/api/v1`), `web.thumb_poll_interval` 2s, `web.max_inflight_files` 200, `web.small_file_max` 2 MiB and `web.lanes` 4 (defaults only: `ServerInfo` from `GET /api/v1/server-info` wins), `web.hash_verify_max_ff_safari` 2 GiB. Server-provided runtime values (`block_size`, `max_lanes`, `small_file_max`, `batch_max_entries`, `max_staged_per_account`, share default expiry, quota, provider list, minimum version) come from API calls, not from files; a `block_size` other than 2 MiB disables upload with a clear message.

## 11. Security notes
No secrets in JS or storage; HttpOnly cookie. Untrusted file names and media types are rendered as text only (Svelte escaping; no `{@html}` anywhere; lint rule). Share tokens in URLs are removed from history with `replaceState` after load and are never logged. The page follows only same-origin API URLs and fixed provider redirects returned by the API (open-redirect guard on `return` parameters: relative paths only).

## 12. Acceptance criteria implemented
AC-3, AC-4, AC-50, AC-51, AC-7 (browser half), AC-47 (display half), AC-12 (page half), AC-57 (approval page), AC-58 (client behaviour), AC-23 (setup-token first run), AC-64 (upgrade message) [v1.0]; AC-62 and AC-13/AC-14 web half [v1.2]. Admin section per confirmed decisions.
**Interaction ACs folded in**: none owned. Participates in AC-77 (share link survives rename/move; the dialog lists links per node, not per path) and AC-70 (after a move the open folder refreshes from the server listing, no client-side path cache).

## 13. Test scenarios
FT-P-09, FT-P-20 (thumbnail state), FT-P-36 to FT-P-39, FT-P-62, FT-P-64, FT-P-65, FT-P-66, FT-P-67, FT-P-68, FT-N-03, FT-N-09, FT-N-10, FT-N-11, NFT-PERF-27, NFT-PERF-28, NFT-RES-29, NFT-SEC-09, NFT-SEC-10, NFT-SEC-19, NFT-SEC-21; v1.2: FT-P-47, NFT-SEC-42, NFT-PERF-29, NFT-RES-LIM-08.

## 14. Component acceptance
- 50 GiB upload in Chrome, Firefox, Safari: heap at most 150 MB; after tab close and re-select at most 10 MiB re-sent; edit mid-upload restarts.
- 50 GiB download: heap at most 200 MB; outcome labelled verified/unverified correctly.
- 0 inline renders of non-allowlisted types; 0 inline scripts in the bundle; CSP header test passes.
- Share and approval pages show the specified identical/mismatch behaviour.

## 15. Risks
1. Vite + wasm-bindgen glue unproven (spike).
2. Firefox/Safari verified path under 2 GiB with bounded heap unproven (S12).
3. `wasm-unsafe-eval` versus the v1.2 CSP target (AC-62).
4. iOS Safari memory ceilings for hashing very large files.
5. Size of `sync-core-wasm`: the split is confirmed, trimming it is a risk-register item.
