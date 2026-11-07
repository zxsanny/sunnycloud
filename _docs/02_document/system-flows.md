# sunnycloud — System Flows

Companion to `architecture.md` (the Architecture Vision is not changed here). Every flow has a Mermaid diagram in `diagrams/flows/flow_<name>.md`; this file holds preconditions, steps, error scenarios and the ACs each flow satisfies.

**Conventions**
- Endpoint paths are indicative (all under `/api/v1`); the OpenAPI document is authoritative. Only `commits:batch` and `changes` are fixed by the solution.
- Durations (settle 2 s, lease 5 min, session TTL 24 h, retention 7 d, tombstones 90 d, code TTL 10 min, long-poll 30 s) are configuration; tests use short values and the injectable clock.
- "Counter" is the per-account `epoch.seq` advance, always the last statement before COMMIT, so feed order equals commit order.
- Every request is scoped to the caller's account (404 across accounts, AC-59) and carries the client version (below the minimum gives an upgrade message, AC-64).
- Session-only operations (approve-device, create-share, delete-account, link-provider) return 403 for device tokens (AC-57).

## Flow Inventory

| # | Flow Name | Trigger | Primary Components | Criticality | Release |
|---|-----------|---------|-------------------|-------------|---------|
| F1 | First-run bootstrap | First start with an empty database | Operator, 12 operator_cli, 01 identity, 11 http_api, PostgreSQL, 16 web_app | High | v1.0 |
| F2 | Sign-in through OIDC or GitHub | User opens login or an invite link | Web/mobile browser, 11 http_api, 01 identity, PostgreSQL, identity provider | High | v1.0 (Apple v1.1) |
| F3 | Desktop device-code login and approval | New desktop daemon | 14 desktop_daemon, 01 identity, 16 web_app, OS keychain | High | v1.0 |
| F4 | Large-file upload | Client has a file above 2 MiB | Client uploader (13 sync_core), 11 http_api, 03 transfer, 02 tree, 07a quota, 04 blobstore, PostgreSQL | High | v1.0 |
| F5 | Small-file upload and batch commit | Client has files up to 2 MiB | Client uploader (13 sync_core), 11 http_api, 03 transfer, 02 tree, 07a quota, 04 blobstore, PostgreSQL | High | v1.0 |
| F6 | Download | User or sync client reads a file | Client, Caddy, 11 http_api, 03 transfer, 04 blobstore | High | v1.0 |
| F7 | Change-feed long-poll sync | Device online | Client, 11 http_api, 05 sync_feed, PostgreSQL | High | v1.0 |
| F8 | Conflict handling | Commit or delete with a stale base revision, name collision | 02 tree, 05 sync_feed, PostgreSQL | High | v1.0 |
| F9 | Desktop watcher cycle | Local file event, first pair connect, feed event | 14 desktop_daemon, LocalIndex, 11 http_api | High | v1.0 |
| F10 | Media pipeline | Commit of a photo or video | 02 tree, 08 media_jobs, 09 worker_supervisor, 10 decoder, 11 http_api (worker listener `:8081`), PostgreSQL | High | v1.0 (GPU v1.1) |
| F11 | Share link create, open, revoke | Owner action or visitor opens link | 16 web_app, 11 http_api, 06 sharing, 03 transfer, PostgreSQL | Medium | v1.0 |
| F12 | Deletion, blob GC, retention, operator restore | Delete or overwrite; timer; operator command | 02 tree, 04 blobstore, scheduler (12), 12 operator_cli, PostgreSQL | High | v1.0 |
| F13 | Account deletion | User request | 07 admin, 01 identity, lifecycle-hook owners (02 tree, 03 transfer, 05 sync_feed, 06 sharing, 07a quota, 08 media_jobs), 04 blobstore (GC) | Medium | v1.0 |
| F14 | Database restore, epoch bump, resync | Operator restores a backup | 12 operator_cli, 05 sync_feed, 04 blobstore, 11 http_api, all clients | High | v1.0 |
| F15 | Scrub | Schedule or operator command | 04 blobstore (scheduler in 12), blob volume, PostgreSQL | Medium | v1.0 |
| F16 | Android camera backup | New photo, periodic job, app open, first sync | MediaStore, WorkManager, 17 android_app (`mobile_backup`), 13 sync_core, 11 http_api | High | v1.0 |
| F17 | Vault | User creates or opens a vault | Web/mobile/desktop client with 19 vault_core, 20 vault_service, 11 http_api | High | v1.2 |

## Flow Dependencies

| Flow | Depends On | Shares Data With |
|------|-----------|-----------------|
| F1 | none | F2 (login state, account), F3 |
| F2 | F1 (first admin) | F3 (account), F11, F13 (sessions) |
| F3 | F2 (approver is signed in) | F7, F9 (device token) |
| F4 | F2 or F3 | F5 (name and conflict rules), F8, F10 (jobs), F12 (blob_gc), F7 (feed counter) |
| F5 | F2 or F3 | F4, F8, F10, F7 |
| F6 | F2, F3 or F11 | F7, F9, F11 (serving rules), F15 |
| F7 | F4, F5 produce events | F9, F14 (resync_required), F16 |
| F8 | part of F4, F5, F12 commit | F7, F12 |
| F9 | F3, F4, F5, F6, F7 | LocalIndex, F14 |
| F10 | F4 or F5 commit | F12 (derived blobs in GC) |
| F11 | F2, F6 | none (no vault share links in v1.2, owner decision 2026-11-05) |
| F12 | F4, F5, F8 | F14 (retention covers backup interval), F13 |
| F13 | F2 | F12 (immediate GC), F10 (jobs erased) |
| F14 | F7, F12 | F9, F16 (client resync) |
| F15 | F4 (blobs exist) | F6, F14 |
| F16 | F2 (mobile sign-in yields a session), F4, F5 | F7, F10 |
| F17 | F2, F4, F11 | none |

---

## Flow F1: First-run bootstrap

### Description
The operator starts a fresh install; the API prints a one-time setup token; the first sign-in that presents it becomes the first admin. `sunnycloud admin` later creates or resets admins.

### Preconditions
- Compose stack and Caddy are up; at least one provider is configured (otherwise the API refuses to start); master key available; migrations applied (by the one-shot `migrate` step).
- No accounts exist.

### Diagram
[flow_first_run_bootstrap.md](diagrams/flows/flow_first_run_bootstrap.md)

### Steps
1. The one-shot `migrate` step runs the migrations as `sunnycloud_migrator` before the API starts (a failed migration rolls back and the API does not start until it is re-run). At start the API checks the schema version and refuses to serve on a mismatch.
2. With zero accounts the API generates a setup token, stores its SHA-256, prints the token once to the log. The web app shows the setup page while zero accounts exist.
3. The operator enters the token and picks a provider; the token is attached to the login state (F2) and never sent to the provider.
4. At the callback an unknown identity with a valid, unused token creates the account with role admin and consumes the token in the same transaction; a session is issued.
5. `sunnycloud admin create|reset|promote|demote|setup-token` (operator host only) can issue a one-time admin invite token, promote or demote an account and revoke an admin's sessions; it prints secrets once and never stores them in clear. The last admin cannot be demoted or removed.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Social login without the token on an empty database | Step 4 | No token in login state | No account, generic "invitation required"; counted toward sign-in rate limit |
| Token reused or wrong | Step 4 | Hash missing or consumed | Refused; operator issues a new token with the CLI |
| Two logins race with one token | Step 4 | Atomic consume | One wins, the other is refused |
| No provider configured | Step 1 | Config validation | Server does not start, message names the missing setting |
| Token lost from log | Step 2 | Operator report | `sunnycloud admin setup-token` issues a new setup token while zero accounts exist, or `admin create` an admin invite |

### ACs and Targets
AC-23, AC-22 (30-minute install), AC-19 (provider subset, at least one). Tests: FT-P-64, FT-P-65, FT-N-20, FT-N-21, FT-N-25.

---

## Flow F2: Sign-in through OIDC or GitHub

### Description
The browser authenticates with the provider; the API validates everything and issues its own opaque session. Apple (v1.1) uses the same flow with a hand-built ES256 client secret, no scopes, no PKCE. Invite redemption and provider linking never match accounts by email.

### Preconditions
- Operator registered the provider app; `redirect_uri` is built from the configured public URL, never from the request Host.
- Provider reachable for new sign-ins; existing sessions need no provider (AC-21).

### Diagram
[flow_sign_in.md](diagrams/flows/flow_sign_in.md)

### Steps
1. `GET login?provider=…&return=/path[&invite=…]`: return must be a relative path (not `//`, no scheme); Host must equal the public URL. API saves state, nonce, PKCE verifier (where supported), provider, return path, invite reference and expiry in a short-lived row bound to a browser cookie, then redirects to the provider.
2. Callback: Host check; state must exist, be bound to this browser, unexpired, unused (consumed atomically) and match the provider in the path. A trusted-state failure redirects to the SPA login route with an error code; an untrusted state returns 400.
3. Token exchange with the code and verifier, 10 s timeout, no redirects followed, link-local and metadata ranges blocked. OIDC: validate signature, issuer, audience, nonce, expiry. GitHub: read `/user` and `/user/emails` (verified flag per address).
4. Look up identity `(provider, subject)`. Known: create session. Unknown: only an invite token (or the setup token, F1) in the login state allows account creation. If an allowlist is configured the provider email must be verified and listed; without an allowlist the invite alone suffices and no email is read. The invite is redeemed single-use in the same transaction as account creation.
5. Provider linking for an existing account is a separate session-only operation that starts the same flow in "link" mode; it never matches by email, and an unverified email is never used.
6. Session: opaque access and refresh tokens (SHA-256 at rest). Refresh rotates the token; four concurrent refreshes with the same token yield one successor; a replay after the 30 s grace revokes the token family within 5 s. Cookie-authenticated state-changing requests need `Origin` or `Sec-Fetch-Site` and must be same-origin (403 otherwise). The browser is redirected to the return path.
7. Mobile apps use the same browser flow in the system browser with PKCE; at the callback the API stores a one-time login code (hashed, short-lived, `login_codes`) and returns it through the custom scheme or universal link; the app exchanges it with the PKCE verifier for a session, which consumes the code. Plain-HTTP server addresses are refused by default.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| The 12 negative callbacks | Steps 1-3 | See "OAuth callback cases" below | Non-2xx, no session, no account, no off-site redirect |
| Provider slow or down | Step 3 | 10 s timeout | Sign-in fails clearly; existing sessions unaffected |
| Unknown identity, no invite | Step 4 | No token in login state | "Invitation required"; no account |
| Invite reused or not for this installation | Step 4 | Atomic redeem fails | Refused; operator issues a new invite |
| Provider email unverified (victim email) | Steps 4-5 | `email_verified` false | Email ignored; 0 accounts created or linked |
| Too many failed steps | Steps 1-3 | 10 per minute per IP | HTTP 429 with Retry-After |
| Refresh token replayed after grace | Step 6 | Token family check | Family revoked, user signs in again |

### ACs and Targets
AC-19, AC-20, AC-21, AC-23, AC-54, AC-55, AC-56, AC-58. Tests: FT-P-62, FT-P-63, FT-N-21 to FT-N-23, FT-N-26, FT-N-28 to FT-N-30, FT-N-34.

---

## Flow F3: Desktop device-code login and approval

### Description
A desktop daemon obtains a sync-only device token through the server's own RFC 8628 grant, approved by the user in a signed-in browser. The device grant is for desktop only; mobile apps sign in through F2 step 7 and get a session.

### Preconditions
- Server reachable over HTTPS; user has an account and can sign in on the web (F2).

### Diagram
[flow_device_login.md](diagrams/flows/flow_device_login.md)

### Steps
1. Daemon requests a code with device name and platform. API stores a DeviceGrant (hashed device code, short user code, requester IP and GeoIP location, expiry 10 min) and returns device code, user code, verification URL and poll interval.
2. Daemon opens the verification URL in the system browser (or shows the code) and polls the token endpoint at the interval.
3. The user, signed in on the web, opens the approval page. It shows device name, requester IP and location next to the approver's; a mismatch is flagged with a warning.
4. The user enters/confirms the user code and approves (session cookie plus CSRF checks; device tokens cannot approve). Wrong codes are counted per approving session (5), per account (20 per hour) and per IP (50 per hour); excess gets 429.
5. The next poll returns a new sync-only device token once; API stores only its hash; the grant closes. Daemon stores the token in the OS keychain, falling back on Linux to a user-only file with a logged warning.
6. Revoking the device in the web app invalidates the token; held long-polls end within 5 s.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| User code expired (over 10 min) | Steps 3-5 | Expiry check | `expired_token`; daemon restarts from step 1 |
| Wrong user code | Step 4 | No grant for code | Counted; 5th wrong attempt for that session blocks it for the window; 429 |
| User denies | Step 4 | Explicit deny | `access_denied`; daemon shows message |
| Polling too fast | Step 2 | Interval check | `slow_down` |
| Device token calls a privileged endpoint | After step 5 | Token scope | 403 |
| Keychain unavailable (Linux) | Step 5 | Store error | File fallback with 0600 permissions and a warning |

### ACs and Targets
AC-20, AC-21, AC-26 (daemon starts without a tray), AC-48 (IPC has no token method), AC-54, AC-57. Tests: FT-P-61, FT-P-66, FT-N-24.

---

## Flow F4: Large-file upload

### Description
A client uploads a file above 2 MiB as fixed 2 MiB blocks over up to four lanes. A block is acknowledged only after fdatasync; the commit publishes the revision in constant time.

### Preconditions
- Authenticated client; target folder exists; quota available.
- File hashed locally (BLAKE3 per block and file). Web: Web Worker slices the file (heap at most 150 MB). Android: shared Rust uploader (F16).

### Diagram
[flow_large_upload.md](diagrams/flows/flow_large_upload.md)

### Steps
1. Client hashes the file and builds the blocklist. If the same node already holds a revision with the same file hash, the server answers "nothing to send" and the commit returns the existing revision (0 bytes).
2. `POST uploads`: server applies name rules (rejects `/`, NUL, `.` and `..` as whole names, control and bidi-control characters; accepts everything else), computes `name_key` for case-insensitive uniqueness, checks the per-account open-session cap, quota and free-space floor, reserves `size` of quota through the 07a quota port, creates the UploadSession (24 h) and a blob row in state `uploading` with a random data key wrapped by the master key. Response: session id, `need_blocks`, lane limit 4.
3. Lanes: up to 4 concurrent `PUT block i`. The server checks length and BLAKE3 against the blocklist, encrypts the block as 64 KiB STREAM chunks (chunk index derived from block index, final flag on the last chunk) and writes at its offset in the final blob path. Writes are grouped into batched fdatasync; the 204 ack is sent only after the group is flushed, then the bit is set in the received-bitmap.
4. Resume: after any failure the client reads the session (bitmap, `need_blocks`) and resends only missing blocks (at most 4 lanes x 2 MiB in flight, under 10 MiB). Web: after a tab close the user re-selects the file; size and `lastModified` must match, otherwise the upload restarts. A desktop or mobile client keeps the session id in its local database.
5. `POST commit` with base revision: 03 transfer checks the bitmap, rolls the 32-byte block hashes up to the file hash (no file bytes re-read) and sniffs the media type, then calls 02 tree once. 02 owns the commit transaction: it calls `AccountGuard::ensure_active` (01) first, runs the F8 checks, inserts the Revision (with `client_meta` when sent), flips the blob to committed, sets the node's current revision, queues the replaced blob in `blob_gc`, adjusts usage and releases the reservation through the 07a quota port (`adjust_used`, `release_reservation`), enqueues media jobs (F10, through the commit-effect port) and advances the counter last through the 05 sync_feed port, which stamps the touched nodes. After COMMIT a notification wakes held long-polls (F7).
6. A reaper expires sessions after 24 h and releases their quota; blobs left in `uploading` and orphan files older than 48 h are swept (F12).

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Quota, session cap or free-space floor exceeded | Step 2 | Reservation check | Refused with a typed error; nothing reserved |
| Invalid name | Step 2 | Name rules | 4xx, name absent from listing |
| Wrong block hash or length (bit flip) | Step 3 | BLAKE3 mismatch | 422, block not stored, client resends |
| Disk full or write error | Step 3 | I/O error | 5xx, block not acked, session kept; client retries with backoff |
| Server `kill -9` during uploads | Step 3 | Block not acked or bit not set | Client resends unacked blocks; committed files never contain zero-filled blocks |
| Session expired or unknown | Step 4 | 404/410 | Client starts a new session |
| Commit with missing blocks | Step 5 | Bitmap check | 409 with `need_blocks` |
| Stale base revision or name collision | Step 5 | F8 | Conflict copy or name conflict |
| File edited mid-upload (client side) | Step 4 | Size or mtime change | Client restarts the upload |
| Database down | Steps 2, 5 | Pool error | 503; clients back off; service recovers within 60 s of the database returning |
| Stalled unauthenticated HTTP/2 connections | All | Connection limits | 1,000 stalled connections add at most 200 MB and slow a 1 GiB upload by at most 10% |

### ACs and Targets
AC-2 (at least 90% of 1 Gbps), AC-3, AC-4, AC-7, AC-38, AC-39, AC-42, AC-43, AC-50, AC-59 (stalled connections). Tests: FT-P-01, FT-P-03, FT-P-05, FT-P-06, FT-N-01, FT-N-03, FT-N-40, FT-N-41.

---

## Flow F5: Small-file upload and batch commit

### Description
Files up to 2 MiB are sent as one staged request each and published together by one idempotent batch commit.

### Preconditions
- Authenticated client; files up to 2 MiB; a client-generated commit ID per batch.

### Diagram
[flow_small_upload_batch.md](diagrams/flows/flow_small_upload_batch.md)

### Steps
1. For each file, one request carries the bytes and BLAKE3; the server encrypts, writes, fdatasyncs, then records a completed upload-session row (staged key chosen by the client, quota reserved, blob `uploading`) and returns the staged key. Staged rows have their own per-account cap. A zero-byte file takes the same path with `block_count = 0`. Many run in parallel over HTTP/2.
2. `POST /api/v1/commits:batch` with commit ID C and entries (staged id, parent, name, base revision), up to a configured maximum.
3. The server first looks up C; if known it returns the stored response unchanged. Otherwise one transaction, owned by 02 tree, processes each entry under a savepoint (name rules, F8 checks, Revision, usage adjustment and reservation release through the 07a port), stores the response under C and advances the counter once for the batch (05 port, last) before committing. Entries fail independently with typed per-entry results.
4. A single small file uses the same path with a one-entry batch.
5. Staged blobs not committed within 24 h are reaped like upload sessions.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Response lost after commit | Step 3 | Client sees a network error | Retry with the same C returns the stored response: no duplicates, no conflict copies |
| Staged id expired or unknown | Step 3 | Per-entry error | Client re-sends that file |
| One entry has an invalid name or stale base | Step 3 | Savepoint rollback of that entry | Other entries commit; client handles the failed one |
| Quota exceeded or staged-file cap reached | Step 1 | Reservation or cap check | Refused for that file |
| Batch too large (more entries than `batch_max_entries`) | Step 2 | Limit check | 422 `batch_too_large`; client splits (a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer) |

### ACs and Targets
AC-40 (10,000 files in 60 s at 20 ms RTT), AC-42. Tests: FT-P-04, FT-N-38.

---

## Flow F6: Download

### Description
The server decrypts a blob on the fly and streams it, including Range requests; clients verify BLAKE3 per block.

### Preconditions
- Caller authorized (session, device token) or holding an open share grant (F11); the revision is committed.

### Diagram
[flow_download.md](diagrams/flows/flow_download.md)

### Steps
1. Client reads revision metadata (size, blocklist, hash).
2. `GET content` with optional `Range`; Caddy forwards with buffering off.
3. API authorizes by account, loads the blob, unwraps the data key with the master key, maps the byte range to 64 KiB chunks and seeks to the first chunk (a 4 KiB range read costs at most 5 ms at p95).
4. Each chunk is read, AEAD-verified (the chunk index and the final flag are in the nonce; the AAD is the blob id, so a chunk cannot move between blobs or positions) and streamed; the response is 200 or 206 with `Accept-Ranges`, `ETag` and a type decision: inline only for image/jpeg, png, gif, webp, avif and video/mp4, webm, with `nosniff`; everything else `attachment` with `nosniff` and a sandbox CSP.
5. Client verifies each block, writes a temp file and renames atomically. Browsers stream through the streaming save API where it exists (heap at most 200 MB) and verify in the browser up to 2 GiB; above that the page says "not verified in the browser".
6. Parallel Range lanes (up to 4) may be used for large files.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Blob corrupt at rest | Step 4 | AEAD tag fails | Stream reset (never a complete response), blob flagged suspect, scrub finding (F15), error logged with IDs |
| Hash mismatch in transit | Step 5 | Client BLAKE3 | Discard temp file, retry the block, then report |
| Connection lost | Step 4 | Transport error | Resume with Range from the last verified block |
| Master key missing or wrong | Step 3 | Unwrap fails | 5xx with an operator message; no wrong bytes |
| Range outside file | Step 2 | Bounds | 416 |
| Revision GC'd or file deleted | Step 3 | Lookup | 404 |

### ACs and Targets
AC-2, AC-4, AC-7, AC-43, AC-47, AC-51. Tests: FT-P-02, FT-P-42, FT-P-68, FT-N-02, FT-N-11, FT-N-12.

---

## Flow F7: Change-feed long-poll sync

### Description
Each device holds a cursor `epoch.seq` for its account, long-polls for changes and downloads what changed. An unknown epoch or a sequence below the purge horizon forces a resync.

### Preconditions
- Device authenticated; cursor known (a new device takes the head cursor first, then lists the tree in pages, so nothing is missed).

### Diagram
[flow_change_feed.md](diagrams/flows/flow_change_feed.md)

### Steps
1. `GET /api/v1/changes?cursor=E.S&wait=30[&limit=N]`.
2. Server validates the credential, the epoch, and that S lies between the purge horizon and the head; otherwise it answers `resync_required`.
3. Changed nodes after S exist: return a page of latest-state items (node id, parent, kind, name, live or tombstone, revision id, size, content hash, `seq`) and the next cursor; a node changed ten times appears once, and a folder tombstone means its whole subtree is deleted. None: hold up to 30 s, woken by the post-commit in-process notification, with a periodic safety recheck of the feed in case a wake-up is missed (no database connection is held while parked; v1.0 runs one API process); on timeout return an empty page with the same cursor.
4. The device compares each item with its LocalIndex to tell create, update, move and delete, applies it idempotently (downloads via F6, applies moves and deletes through its guards; remote deletes go to the OS trash or recycle bin) and persists the new cursor only after applying. The desktop keeps one feed loop and one cursor per account.
5. Revoking a session or device wakes its held requests, which end with 401 within 5 s.
6. Tombstones older than 90 days are purged (F12) and the purge horizon advances; Caddy and the API timeouts exceed the 30 s wait.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Unknown epoch, S below horizon, S above head | Step 2 | Cursor check | `resync_required`; device runs F14 steps 4-7 |
| Device offline over 90 days | Step 2 | S below horizon | Safe full resync |
| Held request cut by a proxy or network | Step 3 | Client timeout | Re-poll with the same cursor |
| Token revoked | Step 5 | Wake on revocation | 401, client stops and asks to sign in |
| Item applied twice after crash | Step 4 | Idempotent apply | No effect |
| Hot counter row contention | Commit | Latency metric | Counter updated last; measured in NFT-PERF |

### ACs and Targets
AC-1 (70% idle long-poll sessions), AC-5 (at most 5 s p95 for up to 10 MB, also under 100 background uploads), AC-6 (paged listings), AC-21, AC-35, AC-41. Tests: FT-P-07, FT-P-08, FT-P-09, FT-P-13, FT-P-14, FT-P-58.

---

## Flow F8: Conflict handling

### Description
The later commit by server order becomes current. A stale edit never corrupts or mixes content: the superseded content is kept as a conflict copy. An edit beats a delete. Names are unique per folder case-insensitively.

### Preconditions
- Each change carries the base revision B the device had; the server serializes per account (feed row lock) so "later" means server order, never client clock.

### Diagram
[flow_conflicts.md](diagrams/flows/flow_conflicts.md)

### Steps
1. Edit with B equal to the current revision: publish a new revision on the same node.
2. Edit with a stale B: the commit still becomes current; the revision it superseded moves (same blob, no copy of bytes) to a new node named `name (conflict copy YYYY-MM-DD HH-MM device)` beside it, where device is the label of the committing device, with a numeric suffix if that name exists; both are stamped in the feed.
3. Edit of a tombstoned node: the node is revived with the edit; no conflict copy (edit beats delete).
4. Delete with B equal to the current revision: tombstone. Delete with a stale B: rejected with `edited_since`; the device re-downloads the file.
5. Commit of a new file whose `name_key` exists in the folder: the commit succeeds and the server assigns the conflict-copy name (`name (conflict copy YYYY-MM-DD HH-MM device)`, same rules as step 2); there is no `name_conflict` on commit and no client retry. Folder create, rename and move onto an existing name are rejected with `name_conflict`. A case-only rename keeps the node ID.
6. Save-in-place by editors arrives as an edit of the same node (F9), so the file ID and share links survive.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Client clock wrong | Step 2 | n/a | Ignored; server order decides |
| Two commits at once | Steps 1-2 | Per-account row lock | Serialized; second sees the first as current |
| Case collision (`A.txt`, `a.txt`) | Step 5 | `name_key` | Neither overwritten; the server gives the second a conflict-copy name |
| Delete of an already deleted node | Step 4 | Tombstone present | Idempotent success |

### ACs and Targets
AC-8, AC-34, AC-42. Tests: FT-P-10 to FT-P-12, FT-P-15, FT-P-57, FT-N-04, FT-N-05.

---

## Flow F9: Desktop watcher cycle

### Description
The daemon turns local file events into uploads and feed events into local writes, with settle time, echo suppression, a mass-delete guard, placeholder and lock handling, and a merge rule for a first connection.

### Preconditions
- Device token stored (F3); sync pair configured with include/exclude patterns; LocalIndex (SQLite) open; root sentinel present.

### Diagram
[flow_desktop_watcher.md](diagrams/flows/flow_desktop_watcher.md)

### Steps
1. Startup: check the root (missing, unmounted or empty versus a non-empty index), load the cursor, run a safety rescan (notify can overflow silently on Windows); rescans repeat periodically and after suspend/wake.
2. Events are hints only: a path becomes dirty; excluded paths are ignored (never uploaded, downloaded or deleted).
3. Settle: the path must be unchanged (size, mtime) for 2 s. Locked or unreadable files are retried with backoff and no CPU spin and upload within 10 s after release; they are never reported as deleted.
4. The daemon decides from the state on disk, not from the event kind: a path that exists is an update of the node mapped to that path in LocalIndex even if the editor replaced it by rename-over, so save-in-place gives one revision on the same file ID with no delete.
5. Evicted iCloud or OneDrive placeholders are detected by OS attributes, counted as skipped and never read, hydrated or deleted.
6. Echo suppression: a file whose file id, size, mtime and hash match the record the daemon wrote at download is not uploaded.
7. Deletes: if the root is missing or empty, or 200 or more deletes arrive in a batch, all deletes are held and the user is asked through tray or settings window; "no" re-downloads the files, "yes" sends the deletes with base revisions. 199 deletes pass.
8. Sync: uploads use F4 or F5 with the base revision from LocalIndex. Feed items (F7) are downloaded (F6) to a temp file inside the sync root via directory handles without following symlinks; names the local filesystem cannot represent (traversal, reserved, bidi, case collisions) are saved under a `%XX`-escaped name and reported; the echo record is written before the atomic rename. Remote deletes go to the OS trash or recycle bin.
9. First sync of a pair (merge): enumerate both trees by case-folded path. Local only: upload. Server only: download. Both with equal hash: link in LocalIndex, 0 bytes. Both different: upload the local content as `name (conflict copy)` and download the server version to the path. Case-colliding local names on a case-sensitive filesystem lose neither file.
10. Settings window changes arrive over the user-only JSON-RPC channel and take effect within 5 s; the channel has no token-returning method. The daemon works with no tray.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Sync root unmounted | Step 1, 7 | Sentinel and emptiness check | Deletes held, user asked, nothing propagated |
| Watcher overflow | Step 1 | Safety rescan diff | Missed changes found by stat walk |
| Windows Defender or AV stall | Step 3 | Read timeout | Retry with backoff |
| Server rejects a name | Step 8 | 4xx | Reported in tray, file skipped |
| Offline | Step 8 | Network error | Queue persists in LocalIndex; resumes on reconnect |
| macOS denies a folder [v1.1] | Step 1 | Permission error | "access denied: allow access in System Settings" within 10 s; other pairs continue |
| 30-minute suspend | Step 1 | Wake event | Rescan plus feed catch-up; converge within 60 s |

### ACs and Targets
AC-15, AC-16, AC-17, AC-18, AC-26, AC-29, AC-31 to AC-35, AC-37, AC-48, AC-66, AC-67. Tests: FT-P-48 to FT-P-60, FT-N-05, FT-N-17 to FT-N-19.

---

## Flow F10: Media pipeline

### Description
A commit queues thumbnail or transcode jobs; a trusted supervisor feeds an isolated decoder with decrypted input fetched from the API; results come back through the API.

### Preconditions
- Postgres `jobs` table (reached only by the API); supervisor can reach the API's worker listener (`:8081`, `backend` network, `/api/v1/worker-jobs/**`) and has no database login; two decoder containers per supervisor (interactive with tmpfs scratch, and general with disk scratch), both `network_mode: none`; limits configured (photos 2 CPU, 2 GiB, 120 s; video 4 CPU, 4 GiB, 40 min).

### Diagram
[flow_media_pipeline.md](diagrams/flows/flow_media_pipeline.md)

### Steps
1. In the commit transaction (F4, F5) 08 media_jobs inserts Jobs for photo and video types through the commit-effect port. Vault files and unsupported types get none. Thumbnail jobs are marked interactive or bulk (open question 3).
2. The supervisor claims a job over the worker listener (worker bearer); 08 runs the claim inside the API with `FOR UPDATE SKIP LOCKED` and a 5-minute lease, which the supervisor extends by heartbeat while it works. One slot is reserved for interactive thumbnails; video and bulk jobs use the other slots. Expired leases return the job to the queue; after 3 attempts it fails.
3. The supervisor fetches decrypted input from the worker listener (`:8081`, worker bearer), writes a scratch file and starts the decoder with the job type and limits. It holds no blob mount, no master key and no database login.
4. The decoder only probes and converts. Photos: `vipsthumbnail` (320 px WebP and 1440 px preview, orientation applied so dimensions swap for orientations 5-8, separate HEIC profile, `VIPS_BLOCK_UNTRUSTED=1`). Video: the decoder returns the probe and the supervisor applies the transcode policy (from `media-shared`). A clip is compliant when it is H.264 High or lower, within the orientation-independent bound (long side at most 1920, short side at most 1080), at most 10 Mbit/s, AAC, SDR BT.709. A compliant clip with its index first is never re-encoded (outcome `skipped_compliant`). A compliant clip with its index at the end gets a lossless remux (copy streams, move the index to the front). Anything else is transcoded by ffmpeg to H.264 High 1080p AAC MP4 with faststart, tone-mapping HDR to SDR. The poster frame is taken at 1 s, or at 10% of the duration for clips shorter than 10 s. ffmpeg reads the decrypted scratch file with protocol and format whitelists.
5. Watchdog: no progress line for 90 s kills ffmpeg and retries once on CPU.
6. GPU [v1.1]: at start the worker test-encodes with NVENC and uses it only if the operator enabled it; any GPU failure reruns the job on CPU.
7. The supervisor posts each output to the worker listener's output resource; the API encrypts it as a new blob, inserts the Derived row and marks the job succeeded. Failures and `obsolete` closes are also reported over the listener. The supervisor's heartbeat updates its `worker_status` row (last seen, image age, decoder probe). Adding supervisors on the same host raises throughput; remote workers are out of v1.0.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Hostile media (SVG named .png, HLS playlist to `/etc/passwd`, 30000x30000 PNG, 20 GB sparse video, truncated HEIC) | Step 4 | Decoder failure or limit kill | Job fails; original untouched and downloadable; no data exposed; the hardened decoder runner kills all descendants and exits after a hostile-class outcome, and Compose restarts it |
| Decoder hangs | Step 5 | No progress for 90 s | Kill, retry once on CPU |
| Supervisor dies | Step 2 | Lease expiry (5 min) | Job claimable again, attempt + 1, failed after 3 |
| Revision superseded or file deleted while queued, and already collected (no longer current or retained) | Step 3 | 404 `revision_obsolete` from the worker listener | Job closed with outcome `obsolete`; a superseded or deleted revision that is still retained is served and processed |
| NVENC missing or failing | Step 6 | Probe or exit code | CPU path |
| Derived storage over target | Step 7 | Corpus metric | Tune CRF or presets; target at most 20% of original bytes |

### ACs and Targets
AC-10 (thumbnail within 5 s at p95; bulk 10,000 in 15 min), AC-11, AC-25, AC-28 [v1.1], AC-43, AC-44, AC-45. Tests: FT-P-20 to FT-P-28, FT-N-06 to FT-N-08.

---

## Flow F11: Share link create, open, revoke

### Description
The owner creates a revocable link with an unguessable token; visitors open it without an account; revocation takes effect within 5 s.

### Preconditions
- Owner has a browser session (device tokens cannot create shares); the target is a file (folders are not shareable in v1.0).

### Diagram
[flow_share_link.md](diagrams/flows/flow_share_link.md)

### Steps
1. `POST shares`: a 256-bit random token; only its SHA-256 is stored, with expiry (default 7 days, capped by the maximum share lifetime setting) and an optional password (Argon2id hash). The URL is shown once.
2. A visitor opens the SPA route for the token, which calls the public API. The server hashes the token and looks it up with no cache older than 1 s. Missing, expired and revoked links return an identical 404 (status, body, headers; median timing within 1 ms through constant-work lookup).
3. Password-protected links answer `password_required` until a correct password yields a short-lived access grant signed with the share grant key. Content is served as in F6 (Range, allowlist, `nosniff`, sandbox CSP).
4. `DELETE share` sets `revoked_at`; the next open fails within 5 s.
5. The link targets the node, so a save-in-place edit (F9) keeps it working; deleting the node makes it behave as not found.
6. Create, revoke and access are written to the audit log (IDs only).

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Wrong password | Step 3 | Hash mismatch | No content; 10 wrong per link per 10 min then 429 |
| Token scanning | Step 2 | 100 invalid tokens per minute per IP | 429 with Retry-After |
| Hostile file type behind a link | Step 3 | Type not on allowlist | Attachment with `nosniff` and sandbox CSP |
| Device token calls create-share | Step 1 | Scope | 403 |
| Target is a folder | Step 1 | Node kind | Refused with a typed error |

### ACs and Targets
AC-12 (opens in 2 s), AC-20, AC-34, AC-47, AC-57. Tests: FT-P-36 to FT-P-38, FT-N-09 to FT-N-11.

---

## Flow F12: Deletion, blob GC, retention and operator restore

### Description
Deletes and overwrites queue blob removal in the same transaction; a GC loop removes blobs after the retention period; the operator can restore by path and time inside that period. Users have no trash.

### Preconditions
- Retention 0 to 90 days (default 7), validated at startup; time is read through the injectable clock (test image only: one offset shared by the API and the supervisor, which reads it over the worker listener at `GET /internal/v1/test/clock-offset`).

### Diagram
[flow_deletion_gc_restore.md](diagrams/flows/flow_deletion_gc_restore.md)

### Steps
1. Delete or overwrite commit (F4, F5, F8): in the same transaction as the tree change the API inserts `blob_gc` rows (due = now + retention) for the replaced or deleted original blobs, tombstones the node (kept 90 days) and releases quota. Derived blobs are regenerable and have no retention: once no revision of that content remains, the derived janitor queues them with `Due::Now`.
2. GC loop (a `Janitor` run by the scheduler of the `sunnycloud` binary, 12; v1.0 runs one API process; no leader, rows are claimed with `FOR UPDATE SKIP LOCKED` or a lease): every configured interval it takes due rows, unlinks the file (a missing file is fine), then removes the owner row (`BlobOwnerCleanup::remove_owner_rows`: the revision row whatever its state, or the derived row; for a deleted file the FK clears `nodes.current_revision_id`) and the blob and gc rows. `sunnycloud admin gc-run` triggers one cycle on demand.
3. The same loop purges tombstones older than 90 days and advances the purge horizon; expires upload sessions after 24 h and releases quota; sweeps orphan blob files and `uploading` blobs older than 48 h.
4. Operator restore: `sunnycloud admin restore --account … --path … --at …` finds the revision current at that time (revision rows live until their blob is collected). If its blob is still present, one transaction re-points the existing revision row (and re-parents it into the original folder, or into `Restored`): it never creates a second revision, because `revisions.blob_id` is UNIQUE; `BlobRegistry` reinstates the blob and clears the gc row in the same transaction, and devices receive an ordinary change; otherwise it reports "beyond retention". The restore result carries a placement: `Original` (the original folder exists) or `Restored` (a `Restored` folder at the account root, created on demand).
5. Retention is at least the backup interval so a restored database never points at removed blobs (F14).

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Retention outside 0-90 | Startup | Config validation | Refused with a clear message |
| Crash between unlink and row delete | Step 2 | Next cycle finds missing file | Row removed, no error |
| Restore target already collected | Step 4 | Blob absent | Reported, nothing changed |
| Restore target's folder deleted | Step 4 | Parent tombstoned | File goes to a `Restored` folder at the root |
| Disk held longer than expected | Step 1 | Metrics | Operator lowers retention |

### ACs and Targets
AC-30, AC-41 (retention covers restore), AC-8 (edit beats delete). Tests: FT-P-12, FT-P-16 to FT-P-18, FT-N-35, FT-N-40.

---

## Flow F13: Account deletion

### Description
A user deletes their own account; access ends at once and all data is removed within 15 minutes.

### Preconditions
- Browser session (device tokens get 403); not the last admin (the last admin cannot be removed).

### Diagram
[flow_account_deletion.md](diagrams/flows/flow_account_deletion.md)

### Steps
1. `DELETE account` with an explicit confirmation. Transaction 1: account state `deleting`; revoke all sessions and device tokens (held long-polls end within 5 s). Commits for the account are refused from now on (`AccountGuard::ensure_active`).
2. 07's `DeletionPurger` drives every `AccountLifecycleHook::erase_batch(account, limit)` in short batches until all report `remaining = 0`: 02 tree deletes revisions and nodes and queues each original blob in `blob_gc` with `Due::Now` (retention does not apply); 03 transfer deletes upload sessions, staged rows, receipts and reservations; 05 deletes the feed row; 06 deletes share links; 07a deletes the quota row; 08 deletes jobs and derived rows (derived blobs `Due::Now`); 20 deletes vault rows (v1.2). Only 01 triggers hooks.
3. The GC sweeper unlinks the files. Finally 01 `finalize_deletion` deletes `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row, and an audit entry is written (IDs only). The same provider identity later is an unknown identity again and needs an invite.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Crash mid-purge | Step 2 | `deleting` state persists | Purger resumes; hook batches are idempotent |
| Very large account | Step 2 | Elapsed time metric | Batches sized to finish within 15 min; alert past 10 min |
| In-flight upload commits | Step 1 | Account state check | Rejected; blob swept |

### ACs and Targets
AC-27 (within 15 min), AC-30 (immediate), AC-21. Tests: FT-P-19.

---

## Flow F14: Database restore, epoch bump and resync

### Description
After a database restore the operator bumps the feed epoch; every device receives `resync_required` and converges without losing files.

### Preconditions
- Database dump and blob backup exist; the master key is mounted (without it data stays unreadable and the server says so at start); retention at least the backup interval.

### Diagram
[flow_db_restore_resync.md](diagrams/flows/flow_db_restore_resync.md)

### Steps
1. Operator stops the stack and restores the database (documented, tested end to end in CI).
2. `sunnycloud admin epoch-bump` assigns a new epoch to every account feed, writes it to the `.epoch` marker on the blob volume, and reconciles: blobs without rows (swept after 48 h), rows without blobs (reported). The API start-up self-check verifies the master key against a stored key-check value (for a restored volume with a confirmed key, `sunnycloud admin keycheck-init` re-creates it). If the bump was skipped, `sunnycloud serve` refuses to start because the database epoch is below the marker, and prints `admin epoch-bump`; `sunnycloud doctor` reports the same as Fail.
3. API starts (it checks the schema version and the epoch first). A device polls with the old cursor and receives `resync_required` (also when its sequence is above the restored head).
4. Device reads the new head cursor, then the tree in pages.
5. Device merges with LocalIndex using the F9 merge rule, and the same rule serves devices offline over 90 days: local only is uploaded, server only is downloaded, equal is relinked, different gives a conflict copy.
6. No delete is sent except through the mass-delete guard; a file deleted after the backup point reappears on devices that no longer have it.
7. The device resumes long-polling from the new cursor.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Restore without the master key | Step 2 | Key-check fails | Clear message "data is unreadable without the master key"; no wrong bytes served |
| Epoch not bumped | Step 2 | `serve` refuses to start (database epoch below the `.epoch` marker) and prints `admin epoch-bump`; `doctor` reports Fail | Run the bump; a cursor with seq above head still triggers resync (safety net) |
| Blob missing for a restored row | Step 2 | Reconciliation | Reported; restore the blob file from backup |
| Device edits during resync | Step 5 | Normal commit path | Conflict copy or normal revision |

### ACs and Targets
AC-41, AC-65, AC-43 (master key), AC-6 (paged listing). Tests: FT-P-13, FT-P-14, FT-P-71, FT-N-33.

---

## Flow F15: Scrub

### Description
A throttled background job re-verifies stored blobs and reports corruption at rest.

### Preconditions
- Schedule and byte-rate cap configured; master key available.

### Diagram
[flow_scrub.md](diagrams/flows/flow_scrub.md)

### Steps
1. Schedule or `sunnycloud scrub run [--full]` starts a run that takes blobs with the oldest `last_scrubbed_at` first; no cursor is stored, so an interrupted run simply continues the same ordering. `sunnycloud scrub status` shows the oldest scrub time, the unscrubbed count and open findings.
2. For each blob (originals and derived) the task reads the ciphertext under a byte-rate cap and pauses while transfer load is high, so transfers keep at least 90% of baseline.
3. It decrypts and verifies every chunk's AEAD tag and recomputes block hashes against the blocklist.
4. A mismatch flags the blob corrupt, records a finding, logs IDs, raises a metric and shows degraded on the health endpoint; downloads of that blob fail cleanly (F6).
5. The run summary is written as a `scrub_summary` audit event and metrics (no table); `scrub run` exits 2 on any mismatch or missing blob. The operator restores the blob file from backup (blob files are immutable and named by ID). Automatic repair is not part of v1.0 (open question 9).

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Bit rot or truncated file | Step 3 | AEAD or hash mismatch | Finding and alert |
| Missing blob file | Step 2 | Open fails | Finding "missing" |
| Scrub overload | Step 2 | Transfer throughput metric | Throttle lowers rate |
| Server stops mid-run | Step 1 | Oldest-first selection | Continues next run |

### ACs and Targets
AC-24, AC-7 (corruption at rest), AC-43. Tests: FT-P-43.

---

## Flow F16: Android camera backup

### Description
New photos and videos are uploaded in the background by one shared Rust uploader, started from every Android entry point.

### Preconditions
- A `sessions` row from the mobile sign-in (F2 step 7), held in a store excluded from platform backups; photo permission granted (full or limited); HTTPS server address; battery optimization exempted on Samsung/Xiaomi.

### Diagram
[flow_android_backup.md](diagrams/flows/flow_android_backup.md)

### Steps
1. Triggers: a MediaStore content-URI trigger, a periodic safety job, app open, and a user-initiated data-transfer job for large first syncs. All call the same Kotlin worker, which calls `run_backup()` in the `mobile_backup` crate of 17 (with 13 sync_core, through UniFFI only; no flutter_rust_bridge). Kotlin owns the Rust uploader handle and sends status to Flutter over a Method/EventChannel; Flutter only displays it.
2. The core reads the last MediaStore generation from the checkpoint DB and asks Kotlin for assets above it (visible assets only under limited access; Android 14 limited access hides the library total, so the UI shows "N photos visible, tap to allow more" with no "of M", and the next run picks up newly picked photos). In v1.0 the Android app only backs up: file browsing and download are out of scope.
3. For each asset it hashes (BLAKE3, saved to the checkpoint DB so a kill re-hashes at most 1%) and uploads through F4 or F5 with bounded memory (at most 150 MB). Each asset has a stable `client_key` derived from device, asset id and hash; the batch `commit_id` is random and saved with its staged keys in the checkpoint before the first stage, so a replayed batch has the same id and retries never duplicate.
4. Same asset id with a new hash (an edit) uploads a new file linked to the original; a new asset id is a new unlinked file.
5. After a reinstall the checkpoint DB is rebuilt from the server listing (device, asset id and hash recorded in `revisions.client_meta`), so already-backed-up assets send 0 bytes.
6. The checkpoint is updated after the commit ack; a kill between commit and checkpoint is healed by the idempotent commit ID.
7. Token refresh: the mobile `sessions` row has a 15-minute access token and a rotating refresh token. The client single-flights refresh (one refresh at a time across the worker and the app) and persists the new refresh token before using it, so a kill between response and use never loses the family. A refresh that fails because the family was revoked stops the backup with a sign-in message.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| OEM kills background work (Xiaomi, Huawei) | Step 1 | Missed trigger | Generation-based catch-up on the next run |
| Process killed 10 times in a 50,000-asset sync | Steps 3, 6 | Checkpoint | Resume; no duplicates |
| No Wi-Fi or metered network | Step 1 | WorkManager constraint | Job waits |
| Quota exceeded | Step 3 | Typed error | Backup paused with a visible message |
| Plain-HTTP server address | Setup | Scheme check | Refused by default |
| Refresh token rejected (family revoked) | Step 7 | 401 on refresh | Backup stops; user signs in again |
| Asset deleted between listing and upload | Step 3 | Read error | Asset skipped and recorded |

### ACs and Targets
AC-9 (Pixel 60 s, Samsung and Xiaomi p95 5 min), AC-46, AC-52, AC-53, AC-54, AC-14 (never writes into a vault). Tests: FT-P-29, FT-P-30, FT-P-32, FT-P-34, FT-P-35, FT-N-26, FT-N-36.

---

## Flow F17: Vault [v1.2]

### Description
An opt-in end-to-end encrypted folder tree. The server stores only ciphertext and wrapped keys; vault files cannot be link-shared in v1.2 (owner decision 2026-11-05, step 6).

### Preconditions
- Release v1.2 after the external review; signed-in client with the shared Rust crypto core (native, WASM, UniFFI); one owner per vault.

### Diagram
[flow_vault.md](diagrams/flows/flow_vault.md)

### Steps
1. Create: the passphrase needs 12 or more characters and zxcvbn score 4 (or a generated passphrase). The client generates a vault key and a one-time recovery key, derives a KEK with Argon2id (64-512 MiB, 3 or more passes, parallelism 1), builds an authenticated header with a key-commitment value and uploads the header with the vault key wrapped under the KEK and under the recovery key to 20 vault_service. The recovery key is shown once.
2. Unlock: fetch the header; reject memory outside 64-512 MiB, passes below 3, parallelism other than 1, chunk size outside 64 KiB-4 MiB, unknown versions and any altered byte; derive and unwrap (at most 1 s native, 2 s in the browser).
3. Upload: a per-file key, chunked XChaCha20-Poly1305 with key-wrap AAD = vault id, file id, format version (the parent is not bound, so moves need no re-wrap); the ciphertext goes through F4 (the server additionally encrypts at rest). Files over 100 MiB (configurable) are refused with a clear message. No thumbnails or transcodes are created. Camera backup never targets a vault.
4. The wrapped file key is stored per revision in `revisions.client_meta`, in the same commit as the revision; there is no separate per-node key table.
5. Download: ciphertext through F6; the client rejects tampered, truncated, reordered or wrong-vault data.
6. Share link: not available for vault files in v1.2 (a revision gets a new file key on every edit, so a link would go stale); owner decision recorded in `canonical-names.md`.
7. Lock: keys are zeroed; the web page runs under a CSP without `unsafe-inline`/`unsafe-eval` and loads no third-party hosts.

### Error Scenarios
| Error | Where | Detection | Recovery |
|-------|-------|-----------|----------|
| Wrong passphrase | Step 2 | Header MAC or unwrap fails | Error on client; retry limits are client side (open question 10) |
| Weak passphrase | Step 1 | zxcvbn | Refused |
| Moved key or ciphertext (swap) | Step 5 | AAD mismatch | Rejected (1,000 of 1,000 swaps) |
| Tampered header | Step 2 | Authentication | Rejected (0 of 10,000 fuzzed accepted) |
| Both passphrase and recovery key lost | Any | n/a | Unrecoverable, stated at creation |
| Memory limit on iOS Safari | Step 5 | Peak memory | Chunked decrypt; 100 MiB opens within 250 MB |

### ACs and Targets
AC-13, AC-14, AC-60, AC-61, AC-62. Tests: FT-P-44 to FT-P-47, FT-N-13 to FT-N-16, FT-N-37.

---

## Flow-to-AC Table

| AC | Flows | AC | Flows |
|----|-------|----|-------|
| AC-1 | F4, F5, F7 | AC-34 | F8, F9, F11 |
| AC-2 | F4, F6 | AC-35 | F7, F9 |
| AC-3 | F4 | AC-37 | F4, F9 |
| AC-4 | F4, F6 | AC-38 | F4 |
| AC-5 | F4, F6, F7, F9 | AC-39 | F4 |
| AC-6 | F7, F14 | AC-40 | F5 |
| AC-7 | F4, F6, F15 | AC-41 | F7, F12, F14 |
| AC-8 | F8, F12 | AC-42 | F4, F5, F8 |
| AC-9 | F16 | AC-43 | F4, F6, F10, F14, F15 |
| AC-10 | F10 | AC-44 | F10 |
| AC-11 | F10 | AC-45 | F10 |
| AC-12 | F11 | AC-46 | F16 |
| AC-13 | F17 | AC-47 | F6, F11 |
| AC-14 | F16, F17 | AC-48 | F3, F9 |
| AC-15 | F9 | AC-49 | Integration summary (updater), no numbered flow |
| AC-16 | F9 | AC-50 | F4 |
| AC-17 | F9 | AC-51 | F6 |
| AC-18 | F9 | AC-52 | F16 |
| AC-19 | F1, F2 | AC-53 | F16 |
| AC-20 | F2, F3, F11 | AC-54 | F2, F3, F16 |
| AC-21 | F2, F3, F7, F13 | AC-55 | F2 |
| AC-22 | F1 | AC-56 | F2 |
| AC-23 | F1, F2 | AC-57 | F3, F11 |
| AC-24 | F15 | AC-58 | F2 |
| AC-25 | F10 | AC-59 | All flows (account scoping), F4 (stalled connections) |
| AC-26 | F3, F9 | AC-60 | F17 |
| AC-27 | F13 | AC-61 | F17 |
| AC-28 | F10 [v1.1] | AC-62 | F17 |
| AC-29 | F9 | AC-63 | Build and supply chain, no runtime flow |
| AC-30 | F12, F13 | AC-64 | All flows (client version check) |
| AC-31 | F9 | AC-65 | F14 |
| AC-32 | F9 | AC-66 | F9 |
| AC-33 | F9 | AC-67 | F9 [v1.1] |

Every AC from AC-1 to AC-67 appears at least once; AC-49 and AC-63 are not runtime flows (see the notes).

## External Integration Data Flows

| Integration | Direction and data | Auth and limits | Failure mode |
|-------------|--------------------|-----------------|--------------|
| Generic OIDC (Google, Authelia, authentik, Keycloak, Pocket ID) | Out: discovery document, JWKS, authorize redirect (state, nonce, PKCE challenge, redirect URI), token request (code, verifier), optional userinfo. In: id token (subject, issuer, audience, nonce, email, `email_verified`), access token | Client secret, PKCE; only operator-configured issuers; no redirects; link-local and metadata ranges blocked; 10 s timeout | Sign-in fails clearly; existing sessions and device tokens keep working |
| GitHub OAuth2 | Out: authorize redirect, token request. In: access token, `/user` (id, login), `/user/emails` (address, `verified`) | Client secret (PKCE where supported); 10 s timeout | Same as OIDC; only a verified email is ever considered |
| Sign in with Apple [v1.1] | Out: authorize redirect (no scopes, no PKCE), token request with a hand-built ES256 client secret from the `.p8` key. In: id token (subject, relay or real email, unverified treated as no email) | `.p8` as Docker secret; no device flow, so desktop uses F3 | Same as OIDC |
| Operator backup target | Out only: database dump, blob volume copy, master key copy (separately), made by operator tooling outside the API | Operator's own credentials; the master key is never in the same backup as the data | Restore per F14; missing master key leaves data unreadable |
| GeoIP database (approval page) | Local file read: request IP to city | None, file mounted into the API | Missing database: location shown as unknown, warning still shown when IPs differ |
| Update source (desktop updater, AC-49) | In: Ed25519-signed update manifest hosted on GitHub Releases, source fixed at build time, never the sync server | Ed25519 public key pinned in the build; the daemon rejects unsigned, other-key and lower versions | Update skipped, daemon keeps running |
| Caddy ACME / TLS and Docker registry | Operator infrastructure outside the application flows | n/a | n/a |

### OAuth callback cases (mock-idp list of 12)

Each case is rejected with a non-2xx response, no session, no account and no off-site redirect (FT-N-23, AC-56). The mock-idp also scripts `valid`, `unverified-victim-email` (AC-55) and a slow provider (over 10 s).

| # | Case | Rejected at | Mechanism |
|---|------|-------------|-----------|
| 1 | Missing state | F2 step 2 | State parameter required |
| 2 | Replayed state | F2 step 2 | State row consumed atomically on first use |
| 3 | Foreign state | F2 step 2 | State not found, or not bound to this browser cookie |
| 4 | Wrong provider | F2 step 2 | Provider stored in state must equal the callback path |
| 5 | Wrong nonce | F2 step 3 | id token nonce compared with the stored nonce |
| 6 | Wrong audience | F2 step 3 | `aud` must equal the configured client id |
| 7 | Wrong issuer | F2 step 3 | `iss` must equal the configured issuer |
| 8 | Absolute return URL | F2 step 1 | Only relative return paths are stored; validated again at redirect |
| 9 | Foreign Host header | F2 steps 1-2 | Host must equal the public URL; `redirect_uri` comes from configuration |
| 10 | Reused one-time code | F2 step 3 | Provider returns `invalid_grant`; the API does not retry or fall back |
| 11 | Wrong code verifier | F2 step 3 | Provider's PKCE check fails; the API treats it as failure |
| 12 | Expired state | F2 step 2 | `expires_at` earlier than now (injectable clock) |

## Open Questions for the Architect

1. **Epoch source.** Glossary says "counter bumped", but a counter stored in a rewound database can repeat a value devices already saw. Proposed: a new random epoch id written by `epoch-bump`, plus `seq` above head also gives `resync_required`. Confirm, and confirm an explicit operator command rather than automatic detection.
2. **Conflict copy mechanics.** F8 re-parents the superseded revision to a new node (no blob refcount). Alternative is a second revision pointing to the same blob with reference counting. Confirm, and the exact name pattern (tests use `name (conflict copy)`; device or time suffix?).
3. **Interactive versus bulk thumbnail lane.** AC-10 needs a reserved lane for non-bulk uploads but nothing defines "bulk". Options: a client hint, or the server's per-account recent-commit rate.
4. **Derived blobs and quota/UI.** Do thumbnails and transcodes count against the user's quota? How does the web app learn a thumbnail is ready (poll, or a feed event)?
5. **Mobile sign-in hand-off.** Use the F3 device grant for Android, or a custom-scheme one-time code after the browser flow? (Resolved: the one-time code, which yields a `sessions` row; the device grant is for desktop only.)
6. **Client metadata field.** F16 needs per-revision client metadata (device, MediaStore asset id, link to original) so reinstall sends 0 bytes and edits are linked; this is an API and data-model addition.
7. **Operator restore when the parent folder is gone** (recreate under a `Restored` folder?), and whether share links may target folders in v1.0 (F11 assumes files only).
8. **Last-admin protection** on account deletion and demotion, and whether deletion requires a recent sign-in.
9. **Scrub repair.** v1.0 reports only; is automatic repair from a device copy wanted later?
10. **Vault failure audit.** Architecture audits "vault failures", but wrong-passphrase attempts are only visible to the client; should the client report them to a server endpoint?
11. **Internal endpoint authentication** between supervisor and API (shared secret, per-job token, mTLS) is not specified.
12. **Remaining gaps from the test spec that touch these flows:** NFC vs NFD names (G-1), GC interval/trigger (G-21), password-required response shape (G-9), client-version mechanism (G-19).

## Resolved questions (2026-11-04)

The open questions listed above are answered in architecture.md section 9 (epoch bump, conflict copy name, bulk lane, quota, thumbnail readiness, mobile sign-in, internal endpoint auth, decoder socket, key-check value) and in `component-decisions.md` (restore into a deleted folder, last admin, share links on files only, scrub summaries as audit events, `%XX` escaping, test clock endpoint). Where the open questions above still show an older proposal (for example question 7), the answers win.
