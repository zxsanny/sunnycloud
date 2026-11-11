# Test Specification — 16 Web App

Component tests complement the system-level scenarios (`02_document/tests/`) and reference them by ID. Tiers: **Unit** = Vitest on pure modules (`renderMode`, resume-record logic, error mapping) with the WASM module loaded as built; **IT** = Playwright (Chromium and Firefox in the `browsers` container, profile `web`) against the real server container with real PostgreSQL behind the release Caddyfile; **Safari** = `safaridriver` on the owner Mac (`H-MAC`; Playwright WebKit is not a substitute and is used only for inline-render checks); **C-LARGE** = 50 GiB runs on `blobs-large` (self-hosted runner). Heap is read through CDP `Performance.getMetrics` / `measureUserAgentSpecificMemory` (Chromium), the Firefox memory report, and `vmmap` footprint for Safari (weak evidence, labelled). Only description interfaces are used (`TransferService`, Worker protocol, `ApiClient`, `renderMode`, `sync-core-wasm` exports). v1.2 vault tests are listed in the traceability table and run only when v1.2 is under test.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|-----------------------------|----------|----------|
| AC-3 | Resume re-sends <= 10 MiB | IT-16-5, AT-16-1 | Covered; system NFT-RES-29, FT-P-67 |
| AC-4 | 50 GiB files; constant-time finish | IT-16-4, PT-16-1 | Covered; system FT-P-67, FT-P-03 |
| AC-6 | Listing 10,000 entries is paged | IT-16-16 | Covered (page half); system FT-P-09, NFT-PERF-10 |
| AC-7 | Hash verification in the browser; unverified label above 2 GiB | IT-16-8, IT-16-18, AT-16-2 | Covered; Firefox/Safari verified path BLOCKED on spike S12 (IT-16-8); system FT-P-68, NFT-SEC-21 |
| AC-10 | Thumbnail state shown and polled | IT-16-15 | Covered (page half); system FT-P-20 |
| AC-12 | Share page opens <= 2 s; identical unavailable page; password | IT-16-10, PT-16-5, AT-16-3 | Covered (page half); system FT-N-09, FT-P-36, FT-P-37 |
| AC-21 | 401 returns the user to sign-in | IT-16-14 | Covered; system NFT-SEC-08 |
| AC-23 | Setup-token first-run page | IT-16-12 | Covered (page half) by the "First-run page" paragraph in the 16 description §5; open points listed there; system FT-P-64, FT-P-78, FT-N-20, FT-N-21 |
| AC-40 | Small files single-request + batch in the browser | IT-16-3 | Covered; system FT-P-04 (client other than browser) |
| AC-47 | Inline only for the allowlist; attachment otherwise | IT-16-1, IT-16-2 | Covered; system FT-N-11, FT-P-39, NFT-SEC-19 |
| AC-50 | 50 GiB upload heap <= 150 MB; resume <= 10 MiB; edit restarts | PT-16-1, IT-16-5, IT-16-6 | Covered; Safari = H-MAC; system NFT-PERF-27, NFT-RES-29, FT-N-03 |
| AC-51 | 50 GiB download heap <= 200 MB | PT-16-2, IT-16-8 | Covered; Safari = H-MAC; system NFT-PERF-28, FT-P-68 |
| AC-57 | Device approval page content | IT-16-11, AT-16-4 | Covered; system FT-N-24, FT-P-66, NFT-SEC-07 |
| AC-58 | Cookie never read by JS; browser supplies Origin/Fetch-Metadata | IT-16-13, ST-16-3 | Covered (client behaviour); system NFT-SEC-09 |
| AC-64 | 426 shows the upgrade message | IT-16-14 | Covered; system FT-N-27 |
| AC-70 | After a move the open folder refreshes from the server | IT-16-16 | Covered (contribution); system FT-N-42 |
| AC-77 | Share dialog lists links per node, survives rename/move | IT-16-16 | Covered (contribution); system FT-P-37 and the 02/06 scenarios |
| AC-13, AC-14, AC-62 | Vault web half [v1.2] | IT-16-19 | v1.2: v1.0 placeholder tested here (FT-P-73); full coverage by system FT-P-47, FT-P-44, NFT-SEC-42, NFT-PERF-29, NFT-RES-LIM-08 |

## Blackbox Tests

### IT-16-1: `renderMode` (unit)
- **Summary**: Inline only for the seven allowlisted types, decided by the server's media type.
- **Traces to**: AC-47
- **Input**: `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `video/mp4`, `video/webm`, `IMAGE/PNG`, `image/png; charset=binary`, `image/svg+xml`, `text/html`, `application/xml`, `text/javascript`, `application/pdf`, `application/octet-stream`, empty string; a file named `x.png` whose media type is `text/html`.
- **Expected**: the seven (and the uppercase and parameter variants) -> `'inline'`; all others including empty -> `'attachment'`; the `x.png` file -> `'attachment'` (extension never used).
- **Max time / Deps**: 1 s; Vitest.

### IT-16-2: Hostile files are never rendered inline
- **Summary**: 0 inline renders of non-allowlisted types in three engines.
- **Traces to**: AC-47
- **Input**: DS-17 uploaded, then opened from the file browser and via a share link in Chromium, Firefox, WebKit; positive controls jpg, png, webp, gif, mp4.
- **Expected**: for html, svg (with script), xml, js, pdf and the GIF89a/HTML polyglot: a download link only, no `<img>`, `<video>`, `<iframe>`, `<object>`, `srcdoc` created for them (DOM observer), 0 script executions (sentinel not set); controls render inline.
- **Max time / Deps**: 5 min; server, `browsers`; system FT-N-11.

### IT-16-3: Small files: staging and one batch commit
- **Summary**: Files up to `small_file_max` use single-request staging and one `commits:batch` with a stable random commit id.
- **Traces to**: AC-40
- **Input**: DS-23 boundary files (2 MiB - 1, 2 MiB, 2 MiB + 1) plus 300 small files; the network is cut once after the batch request; fake `ServerInfo` variant with `max_staged_per_account` 200 and `batch_max_entries` 100.
- **Expected**: files <= 2 MiB use `PUT /api/v1/staged-uploads/{staged_key}` (key = file `client_key`), 2 MiB + 1 uses the block path; batches of <= 100 entries; the retried batch carries the same UUIDv7 `commit_id` and gets the stored response (no duplicate files); `StagedCapReached` -> commit what is staged, then stage more; a closed tab abandons the batch.
- **Max time / Deps**: 2 min; server; proxy log.

### IT-16-4: Large upload engine
- **Summary**: The Worker slices 2 MiB, hashes with WASM, sends blocks on up to 4 lanes with one block per request.
- **Traces to**: AC-4
- **Input**: DS-09 (5 GiB) in `C`; DS-10 in `C-LARGE`; observe network requests; a scripted 409 `BlockConflict`; a repeated upload of known content.
- **Expected**: <= 4 concurrent block PUTs, each exactly one block, no streaming request bodies; session created from the blocklist; server hash equals local; `BlockConflict` handled by the WASM planner (re-read, resend or restart); known content ends with `NothingToSend` and 0 block bytes; `block_size != 2 MiB` in `ServerInfo` disables upload with a clear message; finish time after the last block <= 2 s.
- **Max time / Deps**: 20 min / 4 h; server, `blobs-large`.

### IT-16-5: Resume after tab close
- **Summary**: Re-selecting the same file resumes with at most 10 MiB re-sent.
- **Traces to**: AC-3, AC-50
- **Input**: 5 GiB upload, close the tab at 50%, reopen, re-select the same file; second run re-selects a file with changed `lastModified`; third run a file with the same name but different size.
- **Expected**: first: `Resumed{resent_bytes}` <= 10 MiB, no re-hash from zero (IndexedDB blocklist), `client_key` unchanged; second and third: `Restarted{reason: ChangedSinceLastTime}`; a stale blocklist cannot corrupt (server verifies each PUT hash; tampered record -> 422 then restart); expired session -> `UploadSessionGone`, clear message, record kept per spec.
- **Max time / Deps**: 15 min; server, IndexedDB; system NFT-RES-29.

### IT-16-6: File edited mid-upload
- **Summary**: A change of `size` or `lastModified` before commit aborts and restarts the upload.
- **Traces to**: AC-50
- **Input**: 1 GiB upload; the test replaces the file content at 60% (Playwright file handle rewrite) and, in a second run, only touches `lastModified`.
- **Expected**: before commit the Worker re-reads `size` and `lastModified`, aborts, restarts; the final server content equals the new file; 0 revisions with mixed content. System FT-N-03.
- **Max time / Deps**: 10 min; server.

### IT-16-7: `enqueueUpload` typed errors and queue cap
- **Summary**: Each typed error has a visible message; the queue caps in-flight `File` objects.
- **Traces to**: AC-40
- **Input**: errors injected at the proxy: `InvalidName`, 507 `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`, `FreeSpaceFloor`, `ParentNotFound`, `AccountNotActive`; selection of 100,000 tiny files (DS-01 expanded) with `web.max_inflight_files` 200; a selection of `web.max_selection_files` + 1 files (100,001 with the constant of 100,000, confirmed 2026-11-11, DS-01 expanded).
- **Expected**: each error shows its own message, other jobs continue; at most 200 `File` objects in flight (heap snapshot count); `TooManyFiles` is returned only when the page refuses a selection: a selection of exactly `web.max_selection_files` files (100,000, spike AZ-3934, confirmed by the owner 2026-11-11; description section 10) is accepted, one of `web.max_selection_files` + 1 files is refused before any `File` reaches the queue, no upload starts, and the page shows "This selection has 100,001 files. The web client takes at most 100,000 files at once. For larger folders use the desktop client." (the constant's value is the only thing the owner may change).
- **Max time / Deps**: 10 min; server.

### IT-16-8: Download strategies and labels
- **Summary**: The outcome is verified or unverified and always labelled; a mismatch never ends as a good file.
- **Traces to**: AC-7, AC-51
- **Input**: Chromium with `showSaveFilePicker` (2 GiB and 50 GiB); Firefox and Safari at 1 GiB and 3 GiB; a download with one corrupt block; a revision id of a non-current revision.
- **Expected**: Chromium: streamed, hash checked per block, `VerifiedSaved`; Firefox/Safari above 2 GiB: native `<a download>`, text "not verified in the browser" and the server file hash shown; corrupt block -> `HashMismatch`, partial file discarded where the API allows, never `VerifiedSaved`; non-current revision -> `Forbidden`/`NotFound` (non-admin). BLOCKED: Firefox/Safari up to 2 GiB verified path — the mechanism is fixed by spike S12 and the description names none, so no input or expected heap behaviour can be written yet.
- **Max time / Deps**: 2 h; server; Safari H-MAC; system FT-P-68, NFT-SEC-21.

### IT-16-9: CSP and bundle hygiene
- **Summary**: The served page meets the CSP requirement and the bundle has no inline script or non-self URL.
- **Traces to**: AC-47
- **Input**: `GET /` through Caddy; build output scan; Playwright network log over a full session; CSP violation listener.
- **Expected**: header has `default-src 'self'`, `script-src 'self' 'wasm-unsafe-eval'`, `style-src 'self'`, `connect-src 'self'`, `frame-ancestors 'none'`, `base-uri 'none'`, `form-action 'self'`; 0 inline scripts or handlers in `index.html` and assets (build fails otherwise); 0 requests to hosts other than the origin (and the provider redirect returned by the API); 0 CSP violation reports during sign-in, browse, upload, download and share.
- **Max time / Deps**: 5 min; Caddy from the release compose file (CSP is deployment-owned).

### IT-16-10: Share dialogs and public page
- **Summary**: Files-only share creation and a public page that does not reveal why a link fails.
- **Traces to**: AC-12
- **Input**: share a file (default expiry, custom expiry above the server maximum, password), a folder; open links: valid, expired (test clock), revoked, nonexistent, password-protected with wrong and right password, rate-limited (429).
- **Expected**: no share control on folders; expiry default read from the API and capped at the server maximum; URL shown once with a copy button; list shows links with revoke; the three failure cases render byte-identical DOM text ("link not available") and the same HTTP status; `password_required` shows the prompt; 429 shows retry-after; after load `history.state`/URL no longer contains the token (`replaceState`); preview per `renderMode`, otherwise a download button.
- **Max time / Deps**: 5 min; server, test clock (group `X`); system FT-N-09, FT-N-10.

### IT-16-11: Device approval page
- **Summary**: The page shows requester and approver locations, warns on mismatch, counts down and approves or denies.
- **Traces to**: AC-57
- **Input**: daemon-style device grant from `e2e-consumer-b` (other subnet, DS-32 GeoIP) and from the approver's own subnet; look up by user code, wrong code, expired code (10 min, test clock).
- **Expected**: device name and platform shown; both IP/location pairs side by side; prominent warning on mismatch and none on match; countdown to expiry; approve and deny each call the API once; wrong or expired code shows a clear message and grants nothing; a non-cookie session cannot reach the page.
- **Max time / Deps**: 3 min; server, mock GeoIP, consumer-b.

### IT-16-12: Setup-token first-run page
- **Summary**: On an empty database every signed-out route leads to the two-step first-run page, which carries the setup token to the server only in the login-attempt body.
- **Traces to**: AC-23
- **Input**: empty-DB baseline (`setup_required` true), mock-idp with fresh identities, setup token T1 from the API start log (DS-33); network log in Chromium and Firefox. States, in order:
  1. **Trigger**: open `/login`, an invite link and `/files` signed out.
  2. **First run**: land on `/setup`; click "Continue" with the token T1 typed in.
  3. **Token error**: on step 1 enter a wrong token, "Continue", pick a provider, finish the provider step.
  4. **First run: sign in**: enter T1, "Continue", pick a provider, finish the provider step.
  5. **Replaced and used tokens**: on a fresh empty DB run `sunnycloud admin setup-token` (T2) and enter T1 (replaced) as in 3; after state 4 succeeds, open `/setup` and `/login` signed out (`setup_required` false).
  6. **Transport**: network log and storage dump of states 3 and 4; reload on step 2.
- **Expected**:
  1. All three redirect to `/setup` (the `error` query parameter is kept); the sign-in header "Sign in to sunnycloud" is not shown.
  2. Heading "Set up this server" and the text "This server has no accounts yet. The first person to sign in with the setup token becomes its administrator."; steps "1 Setup token" (current) and "2 Sign in as first admin"; field "Setup token" with placeholder "Paste the token"; help text, the command `docker compose exec api sunnycloud admin setup-token` with a "Copy command" button (clipboard holds exactly that command); footer "The token is valid only while the server has no accounts. Later users join by invite."; no provider button yet; "Continue" makes no network request to any API route.
  3. After the callback (302 to `/login?error=invitation_required`, redirected on to `/setup` with the error kept) the page shows step 1, empty field with `aria-invalid="true"`, the text "This setup token is not valid. A token works once and changes when the server restarts."; 0 accounts, 0 sessions (probe `admin user list`); the text is byte-identical for a wrong, a replaced (state 5) and a reused token; "This account is not invited." is not shown.
  4. After "Continue": step 1 shows a check mark, step 2 is current, text "Now sign in with the account that will be the administrator.", provider buttons equal the API's list in its order, "Your administrator chose these sign-in options."; after the provider step 1 admin account exists and the browser lands on the return path with a cookie session (JS cannot read it); no `/setup` view follows.
  5. The T1 attempt ends exactly as in 3 and 0 accounts exist; after the first admin exists `/setup` redirects to the sign-in page and shows no setup text.
  6. `setup_token` appears in the body of the single `POST /api/v1/auth/login-attempts` (with `provider`, `return_to`, `client_kind` "web") and in no URL, header, cookie, `localStorage`, `sessionStorage`, IndexedDB or console line; a reload on step 2 shows step 1 with an empty field.
  Not asserted until the owner settles the open points in the description: route name beyond `/setup`, `/s/<token>` redirect, trimming and empty field, texts for other error reasons, text when the providers call fails, a back control.
- **Max time / Deps**: 5 min; server, mock-idp, empty-DB baseline (group `X`); system FT-P-64, FT-P-78, FT-N-20, FT-N-21.

### IT-16-13: Cookie and CSRF behaviour of the page
- **Summary**: The page never reads the cookie or adds a token header; the browser supplies the origin headers.
- **Traces to**: AC-58
- **Input**: network log of all state-changing calls in Chromium, Firefox, WebKit; static scan of the source for `document.cookie`.
- **Expected**: every state-changing request has `credentials: same-origin`, an `Origin` header and `Sec-Fetch-Site: same-origin`; no `Authorization` or CSRF-token header; `document.cookie` does not contain the session (HttpOnly) and the source has 0 references.
- **Max time / Deps**: 5 min; server; system NFT-SEC-09.

### IT-16-14: Error handling by code
- **Summary**: Each `ApiError.code` leads to the specified UI.
- **Traces to**: AC-64, AC-21
- **Input**: proxy returns 401, 426 `upgrade_required`, 429 with `Retry-After: 7`, 507 `quota_exceeded`, 500 with `X-Request-Id: r-9`.
- **Expected**: 401 -> sign-in page; 426 -> upgrade message with the server text, no retry; 429 -> retry-after 7 s shown; 507 -> quota message; 500 -> generic message with `r-9`; requests carry `X-Request-Id` and `X-Sunnycloud-Client: web/<version>`.
- **Max time / Deps**: 2 min; proxy fault injection.

### IT-16-15: Thumbnail state and polling
- **Summary**: Pending thumbnails are polled while the tab is visible and failed ones get a placeholder.
- **Traces to**: AC-10
- **Input**: `web.thumb_poll_interval` 200 ms (config mock); listing with `pending`, `ready`, `failed`, `none` items; hide the tab (`visibilitychange`), show it again.
- **Expected**: pending items re-requested about every 200 ms (+/- 50%) until `ready`; no polling while hidden; resumes on visible; `failed` shows the placeholder icon and is not polled; no request storm (<= 1 request per pending item per interval).
- **Max time / Deps**: 1 min; server, DS-03.

### IT-16-16: Listing, move refresh, share links per node
- **Summary**: Large listings page by keyset cursor; the open folder reflects server state after a move; links follow the node.
- **Traces to**: AC-6, AC-70, AC-77
- **Input**: DS-02 (10,000 entries); another client moves the open folder and renames a shared file.
- **Expected**: virtual list keeps <= 200 DOM rows, scrolling to the end shows all 10,000 once (no duplicates across pages); after the other client's move the page, on refresh, shows the new location with no client-side path cache; the share dialog of the renamed file still lists its link; no "select all" across pages without a server-side operation.
- **Max time / Deps**: 3 min; server, second client.

### IT-16-17: Admin section visibility and guards
- **Summary**: The admin section is visible only to admins and server guards still apply.
- **Traces to**: AC-23
- **Input**: admin and plain user sessions; direct navigation to admin routes as the user; demote or delete the last admin; create an invite, revoke it; set the default quota.
- **Expected**: no admin menu for the user and admin routes show "not found"/403 (server guarded); last-admin removal refused and the refusal text shown; invites created single-use with expiry and revocable; health values (readiness, free storage, queue depth, worker count) displayed.
- **Max time / Deps**: 3 min; server.

### IT-16-18: WASM conformance in the browser
- **Summary**: `sync-core-wasm` produces the same hashes as native.
- **Traces to**: AC-7
- **Input**: shared vector file (sizes 0, 1, 2 MiB - 1, 2 MiB, 2 MiB + 1, 6 MiB, 100 MiB) hashed in a module Worker in Chromium, Firefox, WebKit.
- **Expected**: block hashes, rollup and commit ids identical to native vectors; hashing never runs on the main thread (long-task observer shows no task above 100 ms during a 1 GiB hash).
- **Max time / Deps**: 5 min; WASM build.

### IT-16-19: v1.0 build has no vault
- **Summary**: `/vault` shows a placeholder and no vault code ships.
- **Traces to**: AC-13
- **Input**: open `/vault`; list bundle chunks.
- **Expected**: "not available in this version"; no vault WASM or chunk in the build output. System FT-P-73.
- **Max time / Deps**: 1 min.

## Performance Tests

### PT-16-1: Browser 50 GiB upload heap (gate S12)
- **Summary**: Worker plus page heap stays bounded.
- **Traces to**: AC-50
- **Load**: DS-10 (50 GiB), Chromium and Firefox (`C-LARGE`), Safari (H-MAC); sample heap every 5 s; `shape-wan`.
- **Targets / failure**: heap <= 120 MB / above 150 MB; resume re-send <= 10 MiB; hashing keeps up with the link (no idle gap above 1 s between blocks). System NFT-PERF-27.

### PT-16-2: Browser 50 GiB download heap
- **Summary**: Download memory stays bounded and is labelled.
- **Traces to**: AC-51
- **Load**: DS-10 download in Chromium, Firefox, Safari; save-to-disk path as in IT-16-8.
- **Targets / failure**: heap <= 160 MB / above 200 MB; outcome label correct. System NFT-PERF-28.

### PT-16-3: Main-thread responsiveness during hashing
- **Summary**: The UI stays responsive while a 5 GiB file hashes.
- **Traces to**: AC-50
- **Load**: DS-09 upload with scroll and click interactions every second.
- **Targets / failure**: longest main-thread task <= 50 ms / above 100 ms; input latency p95 <= 100 ms / above 200 ms. Targets proposed by this spec.

### PT-16-4: 100,000-file selection
- **Summary**: A huge selection does not exhaust memory.
- **Traces to**: AC-40
- **Load**: 100,000 tiny files selected by folder pick.
- **Targets / failure** (restated 2026-11-11 by the owner, spike AZ-3934; browser memory of all processes): at 100,000 files <= 1.5 GB / above 3 GB and first upload starts <= 15 s / above 30 s; up to 10,000 files the original targets stand (<= 150 MB / above 300 MB; <= 5 s / above 15 s).
- **Measured 2026-11-11 (spike AZ-3934, developer workstation, informational)**: at 100,000 files Firefox 159.0 adds 976 to 1,374 MB and starts after 6.7 to 7.0 s, Chromium 158 adds 838 to 839 MB and starts after 11.2 to 11.4 s, Playwright WebKit (Linux), not Safari, adds 250 MB and starts after 1.4 to 1.5 s; at 10,000 files at most 146 MB and 1.3 s in all three. All meet the restated targets; description section 5 "Selection limit" has the full table.

### PT-16-5: Public share page open time
- **Summary**: A share link opens quickly.
- **Traces to**: AC-12
- **Load**: 100 opens of a 1 MiB image link, `shape-wan`.
- **Targets / failure**: page ready p95 <= 1 s / above 2 s. System FT-P-36.

## Security Tests

### ST-16-1: Untrusted names and types render as text
- **Summary**: Hostile names, media types and error text cannot inject markup or script.
- **Traces to**: AC-47
- **Attack vector**: file names `<img src=x onerror=...>`, `"><script>`, bidi override characters, 255-character names; server error text with markup; share names.
- **Procedure**: create the names via the API; browse, search, share and download them; lint check for `{@html}` in the source.
- **Pass**: 0 script executions, names shown as plain text, 0 `{@html}` usages (lint). **Fail**: any execution or markup rendered.

### ST-16-2: No secrets in browser storage
- **Summary**: No token, password or share password is stored.
- **Traces to**: AC-58
- **Attack vector**: storage inspection after sign-in, share password entry, upload and resume.
- **Procedure**: dump `localStorage`, `sessionStorage`, IndexedDB, Cache Storage; grep for sentinel share password, session value, device code; confirm `localStorage` preferences are in try/catch (storage disabled run).
- **Pass**: 0 sentinels; with storage blocked the app still works. **Fail**: any sentinel or an uncaught storage exception.

### ST-16-3: Cross-origin requests with a valid cookie
- **Summary**: The page's own requests satisfy the server's origin checks (server side is NFT-SEC-09).
- **Traces to**: AC-58
- **Attack vector**: cross-origin form POST and `fetch` from a second origin using the victim's cookie.
- **Procedure**: from `evil.test`, submit state-changing requests; repeat from the app origin.
- **Pass**: cross-origin -> 403; same-origin page actions succeed. **Fail**: any cross-origin success.

### ST-16-4: Open-redirect guard and token hygiene
- **Summary**: `return` parameters accept relative paths only and share tokens leave history.
- **Traces to**: AC-12
- **Attack vector**: `?return=https://evil.test`, `//evil.test`, `/\evil.test`, `javascript:`; share token in the URL, referrer and console.
- **Procedure**: sign in with each value; open a share link and inspect history entries, `document.referrer` of an outgoing link, console and network logs.
- **Pass**: all absolute and scheme-relative values ignored (lands on `/`); token absent from history after load, console, referrer. **Fail**: navigation to another host or token visible.

## Acceptance Tests

### AT-16-1: Upload, close, resume
- **Summary**: A user can interrupt and resume a big upload.
- **Traces to**: AC-3, AC-50
- **Preconditions**: signed in, 5 GiB file.
- **Steps**: (1) drag the file in: progress shows lanes and bytes; (2) close the tab at about 50%; (3) reopen, select the same file: the page shows it resumed, at most 10 MiB re-sent; (4) finish: the file is in the listing with the same hash.

### AT-16-2: Download with outcome label
- **Summary**: The user is told whether the download was verified.
- **Traces to**: AC-7, AC-51
- **Preconditions**: a 3 GiB file; Chromium and Firefox.
- **Steps**: (1) Chromium: save: "verified"; (2) Firefox: save: "not verified in the browser" with the server hash shown; (3) corrupt a block on the server copy via `e2e-ctl`: the page reports a hash mismatch and no saved file is called verified.

### AT-16-3: Share a file
- **Summary**: A link is created, opened anonymously and revoked.
- **Traces to**: AC-12
- **Preconditions**: signed in, one JPEG.
- **Steps**: (1) share with a password: URL shown once; (2) anonymous window: password prompt, then inline image; (3) revoke: within 5 s the link shows "link not available", identical to an unknown token.

### AT-16-4: Approve a desktop device
- **Summary**: The approval page protects against a wrong device.
- **Traces to**: AC-57
- **Preconditions**: a pending device grant from another network.
- **Steps**: (1) enter the user code: device and both locations shown with a mismatch warning; (2) deny: the daemon sees `Denied`; (3) request again, approve from the matching network: the daemon receives its token.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-09, DS-10 | 5 GiB dense, 50 GiB sparse-hybrid | `gen_bigfile.py` | 5 GiB / 800 MB allocated |
| DS-01, DS-02, DS-23 | many small, 10,000-entry folder, boundary sizes | generators | 1 GB |
| DS-03, DS-17 | photos, hostile uploads | generators | 100 MB |
| DS-32 | GeoIP test database | test-data.md | < 1 MB |

**Setup**: `tests/run.sh --profile web`; accounts via invite tokens; `blobs-large` for 50 GiB; short product durations via config mock (thumbnail poll 200 ms); test-clock tests are group `X`.
**Teardown**: close browser contexts (fresh profile per test), purge accounts through `e2e-ctl`, delete downloaded files.
**Isolation**: one account and a fresh browser context per test; no fake API at the browser boundary except the proxy fault injection described per test; all integration runs use the real server container with real PostgreSQL and Caddy.
