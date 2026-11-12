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
Operations used: server info (`GET /api/v1/server-info`), auth (session info, sign-out, providers list with `setup_required`, login attempt; there is no separate setup call, `GET /api/v1/setup` is not registered), tree (list page by `name_key` cursor, create folder, rename, move, delete), uploads (create session, get session, put block, commit, `PUT /api/v1/staged-uploads/{staged_key}`, batch commit), download/range, changes (not used by the page; browser does not sync), shares (create, list, revoke, public open, password unlock), devices (list, revoke, approval lookup/approve/deny), admin (users, invites, quotas, health), thumbnails (state per listing item).

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

No token, password or share password is ever written to browser storage. A 100k-file selection creates 100k `File` objects: the queue caps in-flight objects (`web.max_inflight_files`, default 200), and a selection above `web.max_selection_files` is refused before it reaches the queue (section 5, "Selection limit").

## 5. Implementation Details

**Upload engine (AC-50, AC-3, AC-4)**: files up to `small_file_max` (2 MiB) are staged with `PUT /api/v1/staged-uploads/{staged_key}` (the key is the file's `client_key`) and committed in one `POST /api/v1/commits:batch` of at most `batch_max_entries` entries with a random UUIDv7 commit ID. The batch (commit ID and staged keys) is kept in memory for the life of the tab and reused on every retry, so a retried batch gets the stored response; a closed tab abandons it and the staged rows expire. `StagedCapReached` means: commit what is staged, then stage more. Each large upload uses a random UUIDv7 `client_key` stored with its IndexedDB resume record and reused on every resume. A `409 BlockConflict` is passed to the WASM planner (retry after a session re-read, or restart). `NothingToSend` ends an upload with 0 bytes. Larger files: the Worker reads 2 MiB with `Blob.slice`, hashes each block with the WASM hasher, builds the blocklist, creates the session, then sends blocks with plain `fetch(PUT)` on up to 4 lanes (one block per request; no streaming request bodies). Heap stays at most 150 MB (worker + page). Before commit it re-reads `size` and `lastModified`; any change aborts and restarts (FT-N-03). Resume after tab close: the user re-selects the file; if name, size and `lastModified` equal the IndexedDB record, the Worker reads the session bitmap and sends only missing blocks (at most 10 MiB re-sent); otherwise it restarts. The saved blocklist avoids re-hashing 50 GiB on resume; the server still verifies every block hash on PUT, so a stale blocklist cannot corrupt data. A phone locking or the tab closing ends the upload; the page says so and keeps the resume record. Folder upload is files only (`webkitdirectory`; empty folders only by drag and drop); large trees point to the desktop client (selection limit below).

**Downloads (AC-7, AC-51)**: strategy chosen per browser and size, outcome always labelled.
1. Chromium with `showSaveFilePicker`: stream to disk, hash incrementally block by block, verified at any size.
2. Firefox and Safari up to 2 GiB: verified path (mechanism to be fixed by spike S12: streamed fetch with incremental hash and disk-backed save; no whole-file `Blob` in heap); outcome `VerifiedSaved`.
3. Above 2 GiB on Firefox and Safari: native `<a download>` of the API URL; the page states "not verified in the browser" and shows the server-provided file hash for manual check. Heap at most 200 MB in all three browsers.
Mismatch never ends as a saved "good" file (`HashMismatch`, partial file discarded where the API allows).

**Share dialogs**: files only (no share control on folders in v1.0); create (expiry default from server config shown in the field and capped by the server's maximum share lifetime, both read from the API; optional password), the URL is shown once with a copy button, list with revoke. Public page `/s/<token>`: calls the public API; missing/expired/revoked all render the same "link not available" page; password prompt on `password_required`; inline preview per `renderMode`, otherwise a download button; 429 shows retry-after. The page does not distinguish the failure reasons because the server does not.

**Device approval page** (F3, AC-57; SPA route `/device`, owner decision 2026-11-12; it is the `verification_uri` of the device grant): lookup by user code, shows device name and platform, requester IP/location next to the approver's, a prominent warning on mismatch, expiry countdown, approve/deny. Only a cookie session can approve.

**First-run page** (AC-23; drafted 2026-11-11 from the approved mockup `ui_mockups/web-sign-in.html`, states "First run", "Token error", "First run: sign in", and from 01 §3 and `operator_identity_ops.md`; the owner may overturn the draft). No server behaviour is added: the page uses only `GET /api/v1/auth/providers` and `POST /api/v1/auth/login-attempts`.
- *Route and trigger*: the SPA route is `/setup` (a page-route name chosen here; no source names one, open point 1). Every signed-out SPA route reads the providers answer first; while `setup_required` is `true` it replaces the view with `/setup`, keeping an `error` query parameter. When `setup_required` is `false`, `/setup` redirects to the sign-in page, so it cannot be reached once an account exists. The page has no deep link: a `setup_token` in the URL is ignored.
- *Step 1, "Setup token"* (mockup "First run"): heading "Set up this server"; text "This server has no accounts yet. The first person to sign in with the setup token becomes its administrator."; step list "1 Setup token", "2 Sign in as first admin"; one mono field "Setup token" (placeholder "Paste the token", no autocomplete, no spellcheck); help "The server prints it once in its log at first start. To print a new one, run this on the server:" with the command `docker compose exec api sunnycloud admin setup-token` and a "Copy command" button; button "Continue"; footer "The token is valid only while the server has no accounts. Later users join by invite."
- *Step 2, "Sign in as first admin"* (mockup "First run: sign in"): step 1 shows a check mark, step 2 is current; text "Now sign in with the account that will be the administrator."; the provider buttons exactly as on the sign-in page (only the providers the API lists, in API order) and "Your administrator chose these sign-in options." "Continue" does no server call: no route exists that checks a token alone (`GET /api/v1/setup` is not registered), so the page keeps the value in component memory and shows step 2.
- *What is sent*: picking a provider sends one `POST /api/v1/auth/login-attempts` with `{provider, return_to, client_kind: "web", setup_token}`, `return_to` being the same relative path the sign-in page uses; the browser then follows `authorization_url`. The token travels only in that body: never in a URL, `localStorage`, `sessionStorage`, IndexedDB or a cookie, and not written to the console. A reload or the provider round trip therefore returns the user to step 1.
- *Token error* (mockup "Token error"): `begin` has no token error (01 lists `UnknownProvider`, `InvalidReturnPath`, `HostMismatch`, `RateLimited`), so the token is checked only in `complete`. An unknown, used, replaced or expired token (01 gives no separate outcome) ends as `InvitationRequired`, which the server turns into a 302 to `/login?error=invitation_required` after the provider round trip and with no account created. While `setup_required` is `true` the page maps that reason to step 1 with the field empty (the typed value is gone after the round trip; the mockup's prefilled value cannot be reproduced) and `aria-invalid` set, and the message "This setup token is not valid. A token works once and changes when the server restarts." The wording is neutral because the answer cannot tell the causes apart. With `setup_required` `false` the same reason shows the sign-in page's "This account is not invited." state instead.
- *Success*: a valid token creates the first account with role admin, the callback is a 302 to `return_to` with the session cookie, and the next providers read gives `setup_required` `false`. `RateLimited` on the login attempt shows the generic retry-after message (§5 error handling).
- *Open points (sources silent, nothing invented)*: (1) the route name `/setup` and whether the public share page `/s/<token>` also redirects while no account exists; (2) whether the pasted value is trimmed or format-checked, and what an empty field does on "Continue"; (3) texts for other `/login?error=` reasons while `setup_required` is `true` (`invalid_request`, provider-failure codes); (4) the message does not mention the 7-day token expiry (`setup_token_ttl`); (5) the page text if the providers read itself fails; (6) a way back from step 2 to step 1 is not drawn; (7) an earlier token check (before the provider round trip) is not offered by 01 or 11 (finding 9 in `ui_mockups/DESIGN.md`, 11 open point 8); if E-15 adds one, only the moment of the "Token error" state changes.

**Thumbnails**: listings carry a thumbnail state (`pending|ready|failed|none`); while the tab is visible the page polls pending items every `web.thumb_poll_interval` (2s), stops on `visibilitychange`, and shows a placeholder icon for `failed`.

**Admin section**: visible only when the session profile has the admin role; routes also guarded server-side. Users (list, deactivate/delete; removing or demoting the last admin is refused by the server and shown as such), invites (create single-use with expiry, revoke), quotas (default and per user), health (readiness, storage free, queue depth, worker count from the health endpoint). No billing, no organisations.

**CSRF and cookies**: all state-changing calls are `fetch` with same-origin credentials; the browser supplies `Origin` and `Sec-Fetch-Site`, which the server requires (AC-58). The page adds no token header and never reads the cookie.

**CSP requirement (edge config owned by deployment)**: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'` (WASM only; vault page v1.2 must serve without `unsafe-inline`/`unsafe-eval` and review whether `wasm-unsafe-eval` is acceptable under AC-62), `style-src 'self'`, `img-src 'self' blob:` where needed, `connect-src 'self'`, `frame-ancestors 'none'`, `base-uri 'none'`, `form-action 'self'`, no third-party hosts, no inline handlers. Build fails if the bundle contains an inline script or a non-self URL (allowlist for the string constants of the Svelte runtime, see "WASM packaging" above).

**Selection limit (spike AZ-3934, 2026-11-11; measured on a developer workstation, informational; CONFIRMED by the owner 2026-11-11)**: `enqueueUpload` returns `TooManyFiles` when a selection holds more than `web.max_selection_files` files (100,000, confirmed by the owner 2026-11-11; the in-flight cap `web.max_inflight_files` 200 is unchanged). The count is `input.files.length` for a folder pick and the running count while a dropped folder is walked (refuse at the first file over the limit). The page refuses before any `File` reaches the queue and shows: "This selection has {count} files. The web client takes at most {limit} files at once. For larger folders use the desktop client." (`{limit}` is shown with thousands separators.) Measured with Playwright 1.65.0 on Linux, Firefox 159.0, Chromium 158.0.8127.18 and Playwright WebKit 27.2 (Linux), not Safari, by `setInputFiles` on a real folder of 1 KiB files into a `webkitdirectory` input, the closest scripted match to a folder pick; the first upload request is a `PUT` of the first file. "Memory" is the change in proportional memory (PSS) summed over all processes of the browser instance (baselines: Firefox about 418 MB, Chromium about 173 MB, WebKit about 220 MB; noise about 20 MB): first number right after the selection, second 3 s later. Chromium also gives the page's JS heap through CDP `Performance.getMetrics` (`JSHeapUsedSize`); Firefox and WebKit expose no JS heap figure. Each cell: memory peak / after 3 s; pick to first request at the server.

| Files | Firefox | Chromium (JS heap) | Playwright WebKit (Linux), not Safari |
|------:|---|---|---|
| 1,000 | 21 / 2 MB; 0.12 s | 21 / 26 MB (2.7 MB); 0.15 s | 17 / 14 MB; 0.04 s |
| 10,000 | 142 and 146 / 57 and 56 MB; 0.6 and 0.8 s | 97 and 98 / 107 MB (2.7 MB); 1.1 and 1.3 s | 47 and 48 / 41 and 37 MB; 0.16 to 0.17 s |
| 100,000 | 1,374 and 976 / 566 and 569 MB; 6.7 and 7.0 s | 839 and 838 / 921 and 920 MB (4.9 MB); 11.2 and 11.4 s | 250 and 250 / 222 and 221 MB; 1.4 and 1.5 s |
| 250,000 | 3,471 / 1,463 MB; 17.3 s | 2,297 / 2,295 MB (9.0 MB); 27.8 s | 584 and 575 / 517 and 516 MB; 3.7 and 3.8 s |
| 500,000 | the Firefox main process crashed (SIGSEGV) after 26 s, twice | 4,559 / 4,557 MB (12.1 MB); 57.2 s | 1,128 / 1,013 MB; 7.5 s |

Creating the same counts of empty `File` objects inside the page (no folder pick) costs, for 10,000, 100,000 and 250,000: Firefox 5, 34 and 82 MB with the first request within 1 s (500,000: 146 MB); Chromium 79, 699 and 1,704 MB with the first request after 0.1, 2.1 and 2.1 s (500,000: 3,248 MB); Playwright WebKit 18, 144 and 291 MB with the first request within 0.7 s. In every engine most of the cost is the browser's own per-file work (folder enumeration, and in Chromium native per-`File` state outside the JS heap), not the JS objects. Playwright WebKit (Linux), not Safari, is the lightest and fastest of the three (about 4 times less memory than Chromium and 5 to 8 times faster at 100,000); Chromium is the heaviest and slowest; Firefox is in between but the only one that crashed (500,000). WebKit therefore does not change the confirmed limit or the restated PT-16-4: at 100,000 files all three engines are within 1.5 GB (Firefox peak 1,374 MB, Chromium 839 MB, WebKit 250 MB) and within 15 s (7.0, 11.4 and 1.5 s), and at 10,000 files within 150 MB (at most 146 MB) and 5 s (at most 1.3 s). 250,000 and above stay out: Firefox 17.3 s and Chromium 27.8 s exceed 15 s. Safari itself was not run (`NOT-RUN(HW-DEVICE)`); Playwright WebKit is not a substitute.

Decided 2026-11-11 (owner): the limit is 100,000 files and the PT-16-4 targets are restated (below). The measurements behind the decision: 100,000 is the floor PT-16-4 requires. At 100,000 both engines miss the PT-16-4 memory target (150 MB; failure above 300 MB) when measured as browser memory, and Chromium also misses the 5 s start target (11 s; failure above 15 s); Chromium's JS heap alone is 4.9 MB. 250,000 and above exceed the 15 s start failure line in both engines (Firefox 17.3 s, Chromium 27.8 s), and 500,000 crashes Firefox, so 250,000 is not a candidate. Only about 10,000 files stay under 150 MB in both engines. The owner chose 100,000 with the PT-16-4 targets restated: up to 10,000 files the original targets stand (150 MB, 5 s); at 100,000 files browser memory (all processes) at most 1.5 GB and the first upload within 15 s, failing above 3 GB or 30 s. 100,000 is the largest size under both engines' 15 s line and the PT-16-4 floor.

**Vault page (v1.2, placeholder in v1.0)**: route `/vault` exists behind a build flag and renders "not available in this version"; no vault code is shipped in v1.0 (FT-P-73). In v1.2 it uses 19_vault_core WASM in a module Worker with chunked decrypt, passphrase rules (12 characters, zxcvbn 3), locks by zeroing keys, and offers no share links for vault files (owner decision 2026-11-05: no vault share links in v1.2).

**Key dependencies**: Svelte 5, Vite, TypeScript, generated OpenAPI client (openapi-typescript or equivalent), `sync_core_wasm` (wasm-bindgen CLI `--target web`, single-threaded, CLI version equal to the `wasm-bindgen` pin in `Cargo.lock`, 0.2.130 on 2026-11-11; packaging recorded below), Vitest + Playwright for tests.

**WASM packaging and Worker load (spike AZ-3930, 2026-11-11; measured on a developer workstation, timings informational; Playwright 1.65.0 on Linux: Firefox 159.0, Chromium 158.0.8127.18 and Playwright WebKit 27.2, which is not Safari)**: probe = throwaway Svelte 5.57.3 + Vite 8.3.8 + `@sveltejs/vite-plugin-svelte` 7.3.1 + TypeScript 7.0.2 project, with a copy of the `sync-core-wasm` crate whose one export (`probe_hash_block`) calls `sc_ids::hash_block` (BLAKE3 from the Rust crate, no JavaScript hash library). The probe code is not in the repo.
- **Build integration**: `cargo build --release --target wasm32-unknown-unknown`, then `wasm-bindgen --target web --out-dir pkg`, then `vite build`. The Worker source imports the glue (`pkg/sync_core_wasm.js`) and the binary as an asset URL (`import wasmUrl from '../pkg/sync_core_wasm_bg.wasm?url'`), and the main thread starts it with `new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' })`. No Vite WASM plugin. Config that mattered: `worker.format: 'es'`, `build.target: 'esnext'` (module Worker), `build.assetsInlineLimit: 0` (a `data:` WASM URL would break `connect-src 'self'`).
- **How the Worker gets the WASM URL**: Vite resolves `?url` at build time to the content-hashed path (`/assets/sync_core_wasm_bg-<hash>.wasm`) and writes it into the Worker bundle; nothing is posted to the Worker. The Worker calls `await init({ module_or_path: wasmUrl })`, which `fetch`es and runs `WebAssembly.instantiateStreaming`. The glue's own default (`new URL('..._bg.wasm', import.meta.url)`) is also rewritten by Vite to the same asset, but the explicit argument avoids depending on that. Output: the glue is bundled into the Worker file (`assets/worker-<hash>.js`, 2.9 kB), not into the page bundle; the WASM is a separate hashed file. The static server must send `Content-Type: application/wasm` (otherwise the glue falls back to the slower non-streaming path with a console warning). The probe server sent the CSP header on every response, including the Worker script and the WASM.
- **CSP tokens**: with exactly the CSP below, Firefox, Chromium and Playwright WebKit (Linux), not Safari, each instantiate in the module Worker, returns the expected BLAKE3 values (`abc`, empty input), reports 0 CSP violations on the page and in the Worker, makes 0 WebAssembly calls on the main thread (instrumented) and requests no other host. Needs `script-src 'self' 'wasm-unsafe-eval'` and nothing more (no `unsafe-inline`, no `unsafe-eval`, no `blob:`, no `worker-src`, no cross-origin isolation headers). Served once without `'wasm-unsafe-eval'`, all three fail (5 of 5 runs each); Firefox and Chromium report violation `script-src wasm-eval` in the Worker, Playwright WebKit reports no violation event but throws: Firefox `CompileError: call to WebAssembly.instantiateStreaming() blocked by CSP`; Chromium `CompileError: WebAssembly.instantiateStreaming(): Compiling or instantiating WebAssembly module violates the following Content Security policy directive because 'unsafe-eval' is not an allowed source of script`; Playwright WebKit `CompileError: Refused to create a WebAssembly object because 'unsafe-eval' or 'wasm-unsafe-eval' is not an allowed source of script`. That is the input to risk 3 (Safari itself not run).
- **Size and time (probe crate: only `sc_ids::hash_block`, `opt-level = "s"`, `lto`; the real crate will be larger)**: WASM 30,918 bytes raw, 12,765 gzip -9, 10,723 brotli -11 (`opt-level = 3`: 29,228 raw). First instantiate in the Worker (`init()`: fetch, compile, instantiate; 5 fresh browser contexts, localhost, no cache): Firefox 42, 5, 4, 6, 10 ms (median 6 ms; timer resolution 1 ms), compile of the already fetched bytes 1 to 2 ms; Chromium 3.6, 3.0, 2.7, 3.0, 2.8 ms (median 3.0 ms), compile 0.4 to 0.5 ms; Playwright WebKit 15, 19, 29, 29, 17 ms (median 19 ms; timer resolution 1 ms), compile 0 to 6 ms.
- **Build output scan (AC-3)**: WASM and Worker file names are content-hashed; `index.html` has 0 inline scripts and 0 inline handlers (a seeded inline `<script>` and `onerror=` were both found). The page bundle holds 8 non-self URL strings that the Svelte 5.57.3 runtime never requests (7 `https://svelte.dev/e/<error>` strings and the `http://www.w3.org/1999/xhtml` namespace); the browser made 0 requests to another host. The "non-self URL" build check therefore needs an allowlist for them, or must look only at `src`/`href`/`fetch`/`import` targets, or it fails every build.
- **Engines not run**: Safari (`NOT-RUN(HW-DEVICE)`; Playwright WebKit can load what Safari refuses, so E-26/c10 repeats AC-1 and AC-2 of AZ-3930 on the owner Mac).

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
Build-time constants: API base (`/api/v1`), `web.thumb_poll_interval` 2s, `web.max_inflight_files` 200, `web.max_selection_files` 100,000 (spike AZ-3934, confirmed by the owner 2026-11-11; a selection above it is refused with `TooManyFiles`), `web.small_file_max` 2 MiB and `web.lanes` 4 (defaults only: `ServerInfo` from `GET /api/v1/server-info` wins), `web.hash_verify_max_ff_safari` 2 GiB. Server-provided runtime values (`block_size`, `max_lanes`, `small_file_max`, `batch_max_entries`, `max_staged_per_account`, share default expiry, quota, provider list, minimum version) come from API calls, not from files; a `block_size` other than 2 MiB disables upload with a clear message.

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
1. Vite + wasm-bindgen glue: proven in Firefox 159.0, Chromium 158 and Playwright WebKit 27.2 on Linux (spike AZ-3930, 2026-11-11, packaging in section 5); still open: Safari not run (`NOT-RUN(HW-DEVICE)`; Playwright WebKit is not Safari), and the probe WASM (30.9 KB) is far smaller than the real `sync-core-wasm` (risk 5).
2. Firefox/Safari verified path under 2 GiB with bounded heap unproven (S12).
3. `wasm-unsafe-eval` versus the v1.2 CSP target (AC-62): Firefox 159.0, Chromium 158 and Playwright WebKit 27.2 fail to instantiate without it (measured); Safari not run.
4. iOS Safari memory ceilings for hashing very large files.
5. Size of `sync-core-wasm`: the split is confirmed, trimming it is a risk-register item.
