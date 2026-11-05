# Security Tests

Black-box specifications (no test code). Attacks are mounted through public interfaces: HTTPS API, browsers, the real clients, the operator CLI, and operator-level inspection of the deployed stack (compose, container inspect, blob volume, database dump read-only, logs). Data sets come from `_docs/00_problem/input_data/data_parameters.md`; expected values from `expected_results/results_report.md` ("RR <section> #n"), the acceptance criteria, or `_docs/00_research/raw/b_security.md` (findings "F<n>", marked [research] when the value is not yet in an AC).

## Conventions

- **Accounts**: A and B are ordinary users; C is an admin; E is an unauthenticated attacker; `eve` is a second OS user on a client machine.
- **Canary**: a harness-controlled listener (HTTP and DNS) on the non-internal network plus planted marker files (`SUNNYMARKER-*`, `canary-secret`) on hosts and containers. "0 hits" means 0 requests or 0 byte matches.
- **Uniform response**: equal status, body bytes and header set (ignoring `Date` and request id).
- **T-CLOCK**: injectable server clock of the test image (AC-30): `POST /test/clock` on `:9091` with `{"advance_seconds": N}` (the supervisor reads the offset through `GET /internal/v1/test/clock-offset` on `:8081`); expiry tests advance it. Waits that are config values use short mock durations and assert the production default by reading the config.
- **Status**: `READY` = pass criteria fully defined by an AC, restriction or RR row (RR section "Thresholds added after owner confirmation" included). `BLOCKED — missing data` = a data set, policy or hardware item the verdict needs is still missing; the missing item is named in the scenario and in Gaps.
- Values confirmed by the owner (gap-decisions.md) are used as written in the AC: limits of AC-20, refresh grace 30 s, HSTS one year, inline allowlist of AC-47, name policy of AC-37, decoder limits of AC-25, scrub ≥ 90% of baseline, 4 KiB range read ≤ 5 ms p95, deletion ≤ 15 min.
- Releases: [v1.0] unless tagged otherwise.

---

### NFT-SEC-01: Bootstrap token and first admin [v1.0]

**Traces to**: AC-23, AC-20; RESTRICT-18 | **Data**: empty install, oauth-mock | **Status**: READY (RR Auth #1)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Social login on an empty database without the setup token | Login refused; accounts = 0 |
| 2 | Login with the correct setup token | First admin created; token consumed |
| 3 | 20 concurrent logins from different identities all presenting the token | Exactly 1 account created |
| 4 | Reuse the token afterward | Rejected; accounts unchanged |
| 5 | 1,000 wrong tokens from one IP | 429 with Retry-After after the limit; accounts = 0 |
| 6 | `sunnycloud admin create` and `sunnycloud admin reset <account>` | Works; resulting admin can sign in |
| 7 | Later login by an unknown identity without an invite | No account; with a valid invite → account; the same invite again → rejected |
| 8 | Operator config lists a verified email; unknown identity with that email logs in without an invite, then redeems an invite with an unverified claim, then with a verified claim | No account; denied; allowed (email matched only at invite redemption and only if verified) |

**Pass criteria**: accounts created without token = 0; token usable exactly once (exact); invite single-use (exact); setup token ≥ 128 bits (≥ 22 base64url characters or 32 hex characters).

---

### NFT-SEC-02: Provider email is never used for account creation or linking [v1.0]

**Traces to**: AC-55, AC-23, AC-19; RESTRICT-17, RESTRICT-18 | **Data**: oauth-mock | **Status**: READY (RR Auth #2); Apple-like case [v1.1]

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Mock returns the victim's email with `email_verified=false`, then with the claim missing (Facebook-like), then a GitHub non-primary unverified address | No account created; no link to the victim |
| 2 | Registration closed, attacker identity logs in with the victim's email verified by the mock | No account; no implicit link to the victim's account |
| 3 | Attacker redeems their own valid invite while the mock claims the victim's unverified email | Attacker gets a separate account; victim account unchanged (identity is provider + subject) |
| 4 | Operator list of verified emails contains the victim's email: login without an invite (unverified, then verified claim); invite redemption with an unverified claim, then with a verified claim | Login alone: no account in both cases; redemption unverified: denied; verified: allowed (positive control) |
| 5 | Logged-in attacker links a second provider whose email equals the victim's | Linked to the attacker's own account only |

**Pass criteria**: accounts created or linked from an unverified email = 0 (exact); victim account state hash unchanged.

---

### NFT-SEC-03: Negative OAuth callbacks [v1.0]

**Traces to**: AC-56, AC-20; RESTRICT-17, RESTRICT-20 | **Data**: oauth-mock | **Status**: READY (RR Auth #3; AC-56 now lists 12 cases)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Callback with missing state | 4xx, no session |
| 2 | Replay of a state already used | 4xx, no session |
| 3 | State issued to another browser session | 4xx, no session |
| 4 | State of provider X sent to provider Y's callback | 4xx |
| 5 | id_token with wrong nonce | 4xx |
| 6 | id_token with wrong audience | 4xx |
| 7 | id_token with wrong issuer | 4xx |
| 8 | `return_to=https://evil.example/` | Ignored or rejected; redirects only to a relative path |
| 9 | Foreign `Host` and `X-Forwarded-Host` headers on login and callback | Rejected or ignored; every generated URL uses the configured public URL; code never sent to the foreign host |
| 10 | One-time hand-off code redeemed twice | Second redemption rejected |
| 11 | Hand-off code redeemed with the wrong PKCE verifier | Rejected |
| 12 | State older than its TTL | 4xx |

**Pass criteria**: 12 of 12 rejected (exact); Set-Cookie with a session = 0; accounts created = 0; response bodies contain no stack trace or internal path; a valid login still works after each case.

---

### NFT-SEC-04: Abuse limits per IP for failed sign-in and invalid tokens [v1.0]

**Traces to**: AC-20; RESTRICT-17 | **Data**: invalid inputs | **Status**: READY (RR Auth #6, RR Thresholds #10; AC-20 default: 10 failed sign-in steps per minute per IP)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | 11 failures within 60 s from IP1 (bad state, bad code, invalid bearer token) | The 11th and later get 429 with integer Retry-After > 0 |
| 2 | Same time from IP2 | Not limited |
| 3 | After Retry-After elapses | Request evaluated normally |
| 4 | 50 successful logins/min from IP1 | Never limited (only failures count) |
| 5 | Spoof `X-Forwarded-For` from an untrusted peer with 1,000 random values | Bucket unchanged; limit still hit at failure 11 |
| 6 | Rotate 1,000 source addresses inside one IPv6 /64 | One shared bucket; limit hit at failure 11 |

**Pass criteria**: 429 starts at failure 11 (exact); collateral limiting of IP2 = 0; spoofed headers reset the limit 0 times; 429 responses carry Retry-After.

---

### NFT-SEC-05: Device-approval attempt limits and user-code guessing [v1.0]

**Traces to**: AC-20, AC-57; RESTRICT-19 | **Data**: device grant, 2 accounts | **Status**: READY (RR Thresholds #10; AC-20 defaults: 5 wrong attempts per approving session, 20 per account per hour, 50 per IP per hour)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | One approving session submits 5 wrong user codes, then a 6th | 6th → 429 with Retry-After |
| 2 | During the lockout submit a correct code | Also 429 |
| 3 | Open a second session of the same account | Allowed until 20 wrong attempts in the hour, then 429 for all its sessions |
| 4 | Many accounts from one IP (IPv6: one /64) | 429 from the 51st wrong attempt within the hour |
| 5 | Attacker account makes 1,000 guesses against a victim's pending code over an hour | 0 approvals; guesses evaluated by the server ≤ 20 per hour for that account |

**Pass criteria**: approvals of a pending code by a wrong guess = 0; 429 with Retry-After beyond each limit; a user code is never issued twice while pending.

---

### NFT-SEC-06: Share-password attempt limits and hash-cost DoS [v1.0]

**Traces to**: AC-20, AC-12 | **Data**: 50 password-protected links | **Status**: READY (RR Thresholds #10; AC-20 default: 10 wrong share passwords per link per 10 min)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | 10 wrong passwords against link 1 within 10 min, then an 11th | 11th → 429 with Retry-After for that link only |
| 2 | Open link 2 meanwhile | Not affected |
| 3 | 200 wrong attempts per minute spread across the 50 links for 5 min while an authenticated client lists a folder repeatedly | Listing p95 ≤ 2× its baseline p95 [research F18] |

**Pass criteria**: 429 with Retry-After from the 11th wrong attempt (exact); other links unaffected; API p95 ≤ 2× baseline; correct password inside the limit still opens the link.

---

### NFT-SEC-07: Device-grant phishing controls and device-token scope [v1.0]

**Traces to**: AC-57, AC-21; RESTRICT-19 | **Data**: device grant | **Status**: READY (RR Auth #4, #5)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Daemon requests a code from IP X; user opens the approval page from IP Y | Page shows both IPs and coarse locations, a mismatch warning, and the device name marked unverified |
| 2 | Same IP for both | No mismatch warning |
| 3 | Approve at 590 s and at 601 s after code issue | 590 s accepted; 601 s rejected as expired |
| 4 | Poll the device code after expiry, and after a successful exchange | `expired_token` / rejected; a device code yields a token once |
| 5 | Approve via GET with query parameters, and via POST without CSRF proof | Not approved |
| 6 | Load the approval page in a cross-origin frame | Blocked (`frame-ancestors 'none'` or equivalent) |
| 7 | With a device token call approve-device, create-share, delete-account, link-provider (and vault metadata endpoints [v1.2]) | HTTP 403 on each |
| 8 | With the device token call sync operations | 200 (positive control) |

**Pass criteria**: warning shown on mismatch = yes (exact); user-code TTL ≤ 10 min; 403 on 4 of 4 forbidden endpoints; device token for sync works.

---

### NFT-SEC-08: Revocation within 5 s including live streams [v1.0]

**Traces to**: AC-21; RESTRICT-19 | **Data**: device session, web session | **Status**: READY (RR Auth #7)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Revoke a device while its long-poll (`wait=30`) is parked | Stream closed ≤ 5 s |
| 2 | Revoke while a 1 GiB upload and a 1 GiB download are running | Both cut ≤ 5 s |
| 3 | Use the revoked access token, refresh token and cookie on 10 endpoints | 401 each |
| 4 | Revoke one device; use another device of the same account | Other device unaffected |
| 5 | "Revoke all sessions" | All streams of the account cut ≤ 5 s |
| 6 | Revoked desktop daemon | Stops syncing, shows a re-login state; server accepts 0 blocks from it after 5 s |

**Pass criteria**: time from revoke to cut ≤ 5 s over 20 repetitions (maximum, not median); post-revocation successful calls = 0.

---

### NFT-SEC-09: Cross-origin requests with a valid cookie (CSRF) and CORS [v1.0]

**Traces to**: AC-58; RESTRICT-11, RESTRICT-19 | **Data**: OpenAPI | **Status**: READY (RR Auth #8, RR Second-round #7)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Generate from OpenAPI one request per state-changing operation with a valid cookie, `Origin: https://evil.example`, `Sec-Fetch-Site: cross-site` | HTTP 403; state unchanged on re-read |
| 2 | Same with a sibling-subdomain Origin | 403 |
| 3 | Same-origin request (positive control) | 2xx |
| 4 | Bearer-authenticated request without Origin (desktop and mobile) | 2xx |
| 5 | For every GET operation compare state before and after | No change |
| 6 | Preflight `OPTIONS` from a foreign origin on `/api` | No `Access-Control-Allow-Origin`; no credentials allowed |
| 6b | Every state-changing operation with a valid cookie and neither `Origin` nor `Sec-Fetch-Site` | HTTP 403; state unchanged |
| 7 | Add an operation to the OpenAPI fixture | Generator includes it; CI fails if its test is missing |

**Pass criteria**: 403 on N of N state-changing operations, both for cross-origin requests and for requests lacking both headers (exact); GET operations that change state = 0; CORS allow headers on `/api` = 0 (the public share download route may allow `*` without credentials).

---

### NFT-SEC-10: Refresh-token race and replay [v1.0]

**Traces to**: AC-58, AC-21; RESTRICT-19 | **Data**: refresh token | **Status**: READY (RR Auth #9; reuse grace is 30 s)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Send 4 concurrent refreshes with the same token | One distinct successor token across the 4 responses; family not revoked |
| 2 | Use the successor | Works |
| 3 | Replay the old token at 5 s and at 25 s (inside the grace) | Same successor returned; family not revoked |
| 3b | Replay the old token at 31 s | 401; family revoked: successor also rejected ≤ 5 s |
| 4 | Desktop daemon with 4 lanes and an expired access token | Server sees exactly 1 refresh call |

**Pass criteria**: distinct successors = 1 (exact); revocation after replay ≤ 5 s; a revoked family accepts 0 calls.

---

### NFT-SEC-11: Transport, cookie and token properties [v1.0]

**Traces to**: AC-54, AC-12; RESTRICT-11, RESTRICT-16, RESTRICT-22 | **Data**: deployed stack | **Status**: READY (restrictions: HSTS max-age one year)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | TLS scan | TLS 1.3 negotiated; TLS 1.1 and 1.0 refused; ALPN `h2`; no `h3` in Alt-Svc |
| 2 | Request port 80 | Redirect to HTTPS; no API content served |
| 3 | Inspect session cookie | `__Host-` prefix, Secure, HttpOnly, SameSite=Lax, no Domain, Path=/ |
| 4 | Inspect HSTS | Present on all HTTPS responses with `max-age` ≥ 31,536,000 |
| 5 | Collect 10,000 issued session tokens | ≥ 128 bits each, 0 duplicates |
| 6 | Dump the database for 1,000 live tokens | 0 plaintext tokens; only hashes |

**Pass criteria**: each step matches (exact); plaintext tokens in the database = 0.

---

### NFT-SEC-12: Apps refuse plain-HTTP sign-in [v1.0 Android and desktop, v1.1 iOS]

**Traces to**: AC-54; RESTRICT-22 | **Data**: mobile app, daemon, server on `http://` | **Status**: READY (RR Auth #16)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Enter `http://192.168.1.5:8080` (non-loopback) as the server in the Android app, then in the daemon login | Sign-in refused by default |
| 2 | Capture traffic during step 1 | 0 packets with credentials, OAuth code or device code in clear text |
| 3 | HTTPS server answers with a redirect to `http://` | Client refuses to follow |
| 4 | HTTPS server with an expired or self-signed certificate | Refused; no bypass without explicit trust action |

**Pass criteria**: sign-ins completed over plain HTTP = 0; cleartext credential bytes on the wire = 0.

---

### NFT-SEC-13: Two-account matrix over every operation and ID parameter [v1.0]

**Traces to**: AC-59; RESTRICT-07, RESTRICT-11 | **Data**: OpenAPI | **Status**: READY (RR Auth #10); step 4 asserts only 'never succeeds, 403 or 404' because the exact code for admin-only operations is not specified (G-S6)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | A creates one resource of every kind (node, folder, upload session, share, device, session, job, thumbnail, transcode, invite) | IDs recorded |
| 2 | For each operation and each ID parameter (path, query, body, header) B calls with A's ID | HTTP 404; response uniform with a call using a random nonexistent ID |
| 3 | Compare A's data after every call | Unchanged |
| 4 | Admin-only operations with A's credentials | 403 or 404, never 2xx |
| 5 | Add an operation without matrix coverage to the OpenAPI fixture | CI fails |

**Pass criteria**: successful cross-account calls = 0 (exact); non-404 results = 0; foreign-vs-nonexistent responses differ = 0; CI fails on uncovered operations.

---

### NFT-SEC-14: Cross-tenant paths beyond the matrix [v1.0]

**Traces to**: AC-59, AC-7; RESTRICT-07, RESTRICT-11 | **Data**: two accounts, many-small | **Status**: READY (`b_security` F23, F24, F26)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | B commits a new file whose blocklist claims A's block hashes without sending bytes | Server lists all blocks as missing; commit rejected |
| 2 | B sends wrong bytes for a claimed hash | Rejected by server-side BLAKE3 |
| 3 | B commits against A's upload session; moves a node into A's folder; creates a share for A's file | 404 each |
| 4 | B requests blob, thumbnail, transcode by raw ID | 404 |
| 5 | B sends A's change cursor; B reads the feed (1,000 entries) | `resync_required` or B's own feed only; foreign IDs in feed = 0 |
| 6 | B sends a body with `account_id` of A | Ignored; resource created under B |
| 7 | A and B interleave 10,000 requests over a pool of 5 database connections | Every response belongs to its requester; foreign data in responses = 0 |
| 8 | A fills its quota; B uploads | B unaffected |

**Pass criteria**: foreign bytes obtained by B = 0; all cross-account calls 404 (or `resync_required` for cursors); B's files with A's content = 0.

---

### NFT-SEC-15: Stalled unauthenticated HTTP/2 connections [v1.0]

**Traces to**: AC-59, AC-1, AC-2; RESTRICT-11, RESTRICT-12 | **Data**: 1,000 stalled connections, 1 GiB upload | **Status**: READY (RR Auth #11)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Baseline: server RSS; authenticated 1 GiB upload time T0 (median of 3) | Recorded |
| 2 | Open 1,000 connections that send incomplete HEADERS or DATA and stall; repeat with HTTP/1.1 header drip | RSS rise ≤ 200 MB |
| 3 | Upload 1 GiB while they are open | T1 ≤ 1.1 × T0 |
| 4 | CONTINUATION flood, rapid reset, empty-DATA flood from one host | Process alive (same PID, uptime unbroken); RSS rise ≤ 200 MB; T1 ≤ 1.1 × T0 |

**Pass criteria**: RSS increase ≤ 200 MB; upload delay ≤ 10%; server restarts = 0.

---

### NFT-SEC-16: Share tokens are hashed, random and absent from logs [v1.0]

**Traces to**: AC-12; RESTRICT-11 | **Data**: 1,000 share links | **Status**: READY (RR Sharing #1, #4)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Create 1,000 links; dump the database | No row contains a token or its base64/hex forms; stored values are hashes |
| 2 | Inspect 10,000 tokens | ≥ 128 bits each, 0 duplicates |
| 3 | Open links; grep API and Caddy logs for each token | 0 occurrences |
| 4 | Link created without expiry | `expires_at` = creation + 7 days ± 1 min; configurable |
| 5 | Inspect response headers of a share download | `Cache-Control: private, no-store`, `Referrer-Policy: no-referrer` |
| 6 | Password links | Password stored only as a salted slow hash |

**Pass criteria**: plaintext tokens in the database and logs = 0 (exact); link opens ≤ 2 s with bytes equal to the original.

---

### NFT-SEC-17: Missing, expired and revoked links look identical; enumeration fails [v1.0]

**Traces to**: AC-12, AC-20 | **Data**: one file, share tokens | **Status**: READY (RR Sharing #2, #3; RR Thresholds #10: 100 invalid share tokens per minute per IP)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | 1,000 requests each for a random missing token, an expired token (T-CLOCK) and a revoked token, interleaved | All HTTP 404 with a uniform response |
| 2 | Compare median latency of the three classes | Pairwise difference ≤ 1 ms |
| 3 | Revoke a live link, poll every 100 ms | 404 within 5 s |
| 4 | 100,000 random token guesses from one IP at 200 per minute | 0 hits; 429 with Retry-After from the 101st invalid token within a minute |

**Pass criteria**: status, body and headers identical across classes (exact); median timing delta ≤ 1 ms; revocation ≤ 5 s; successful guesses = 0.

---

### NFT-SEC-18: Range abuse and download amplification [v1.0]

**Traces to**: AC-12, AC-2 | **Data**: big-file 5 GiB shared by link | **Status**: READY (`b_security` F19; a per-link connection cap is not an AC and is not tested)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | `Range: bytes=0-0,1-1,...` with 1,000 ranges on a share and on an authenticated file | Single range served, full 200, or 416; never a multipart of 1,000 parts |
| 2 | Overlapping and unsorted ranges | Rejected or ignored |
| 3 | 4 KiB range at 1,000 random offsets | Correct bytes each |
| 4 | Normal download throughput after the above | ≥ 112 MB/s |

**Pass criteria**: bytes in any response ≤ file size (exact); multipart responses = 0; throughput regression = 0.

---

### NFT-SEC-19: Hostile uploads are never rendered inline [v1.0]

**Traces to**: AC-47, AC-12; RESTRICT-11 | **Data**: uploads-hostile, plus jpeg, png, gif, webp, avif, mp4, webm | **Status**: READY (RR Sharing #5, RR Thresholds #8)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Upload html, svg, xml, js, pdf and the polyglot GIF/HTML; also html bytes named `.png` with `Content-Type: image/png`, and PNG bytes named `.html` | Accepted as data |
| 2 | For each file fetch via authenticated download, share download, preview/thumbnail endpoint, a Range request, HEAD | Non-allowlisted: `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`, CSP with `sandbox` |
| 3 | Fetch the 7 allowlisted types (image/jpeg, png, gif, webp, avif, video/mp4, video/webm) | Inline allowed, `nosniff`, correct media type |
| 4 | Navigate Chrome and Firefox to each URL with a canary script in the payload | No script runs; no document of type html/svg/xml/pdf renders |
| 5 | Thumbnail endpoint for the SVG | Never returns SVG; a WebP/JPEG or 404 |
| 6 | Error responses on these routes | Also `nosniff` |

**Pass criteria**: inline renders of non-allowlisted types = 0 over 6 types × 5 endpoints × 2 browsers (exact); canary executions = 0; missing headers = 0.

---

### NFT-SEC-20: Upload protocol abuse [v1.0]

**Traces to**: AC-4, AC-7, AC-38, AC-39; RESTRICT-09, RESTRICT-11 | **Data**: many-small, big-file | **Status**: READY (`b_security` F25, F26)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Declare 50 GiB + 1 byte | Refused at session creation with a clear message |
| 2 | Send a block larger than 2 MiB | 4xx |
| 3 | Send a block whose `X-Blake3` does not match the body | Rejected; block not counted as received |
| 4 | Out-of-range or duplicate block index; second write of an acknowledged block with different bytes | 4xx; stored block unchanged |
| 5 | Commit with an incomplete received set | 409/4xx; no file created |
| 6 | Body longer than the declared length; request smuggling variants | 4xx; connection closed |
| 7 | Reuse a session after commit | 404/409 |
| 8 | Declared size above remaining quota | Refused at session start, not after transfer |

**Pass criteria**: committed files with zero-filled or foreign blocks = 0; corrupted stored blocks = 0; the server stays up and serves a normal upload afterward.

---

### NFT-SEC-21: Corruption in transit is detected [v1.0]

**Traces to**: AC-7; RESTRICT-11 | **Data**: big-file 5 GiB | **Status**: READY (RR Transfer #1, #3; RR Second-round #8: browsers verify up to 2 GiB)

**Fault injection**: a bit-flip proxy on the plain hop between Caddy and the API flips one bit in 1 of every 100 block bodies (uploads), and on the response path in 1 of 100 blocks (downloads); the blob store itself is untouched.

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Upload 5 GiB | Flipped blocks rejected and re-sent; final hash equals source |
| 2 | Download with desktop and Chrome | Corrupted block detected; refetched, or download fails; no finalized file with wrong content |
| 3 | In Firefox and Safari download a 1 GiB file and a 3 GiB file; for each, run once clean and once with the response path flipping one bit in one block | 1 GiB: hash verified in the browser, the flipped run is detected and the file is not saved as good; 3 GiB: the page says "not verified in the browser" and never shows a verified state |

**Pass criteria**: undetected corrupted bytes in a finalized file = 0 for desktop, Chrome, and Firefox/Safari up to 2 GiB (exact); the 3 GiB browser download shows "not verified in the browser" in 100% of runs; stored blob hash equals source.

---

### NFT-SEC-22: Names and paths in the API; path traversal [v1.0]

**Traces to**: AC-42, AC-37, AC-7; RESTRICT-07 | **Data**: tricky-names, traversal payload list | **Status**: READY (AC-37 name policy; RR Thresholds #11)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Create nodes named `../x`, `a/../../b`, `/etc/x`, `.`, `..`, a name with NUL, a name with a control character, a name with U+202E | Refused (4xx): contains `/`, is `.` or `..`, NUL, control or bidi-control character |
| 1b | Create nodes named `..\x`, `C:\x`, `CON`, `NUL.txt`, `name.` (trailing dot), `file:stream`, a name differing only by case | Accepted and stored verbatim; never interpreted as a path |
| 2 | Request 40 traversal URLs (`..%2f`, `%2e%2e/`, `..;/`, double-encoded, backslash, `%00`) on file content, thumbnail, SPA static assets and share routes | 400/404 only |
| 3 | Download a file named with `"`, CR LF and non-ASCII characters | `Content-Disposition` uses safe encoding; header count unchanged; no injected header |
| 4 | Create `A.txt` then `a.txt` in one folder; NFC then NFD of one name | Second refused or renamed; first untouched |

**Pass criteria**: refused names accepted = 0 and accepted names refused = 0 (exact, per AC-37 policy); files created or changed outside the blob root and canary directory = 0; responses containing host file content (`root:x:0:0`, canary bytes) = 0; header injection = 0; case-colliding overwrites = 0.

---

### NFT-SEC-23: Encryption at rest on disk; wrong or missing master key [v1.0]

**Traces to**: AC-43, AC-2; RESTRICT-08, new restrictions bullet (derived blobs encrypted) | **Data**: big-file 5 GiB seeded with markers every 64 MiB; one 1 MiB text file; photos and clip-1080-sdr for derived blobs | **Status**: READY (RR Encryption #1; AC-43: thumbnails and transcodes are encrypted like every other blob)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Upload the files; scan all blob files, staging area, Postgres data and WAL, backup copies, worker scratch directory after the jobs end | Marker matches = 0 |
| 2 | Compute byte entropy of the blob files, including thumbnails and transcodes generated from photos and clip-1080-sdr | ≥ 7.99 bits/byte; plaintext JPEG/WebP/MP4 signatures (`FFD8FF`, `RIFF....WEBP`, `ftyp`) at blob start = 0 |
| 3 | Upload the same file twice | Ciphertexts differ; 0 identical blob files |
| 4 | Scan the data volume, dumps and backups for master-key bytes | 0 hits |
| 5 | Start with a wrong master key, then with none | Server refuses or every read fails with a clear log message; 0 bytes of plaintext served; no 2xx with garbage |
| 6 | Restore the correct key | Service resumes; hash-set unchanged |
| 7 | Check that the install guide states loss of the key is unrecoverable and describes its backup | Both present |

**Pass criteria**: plaintext markers on disk = 0 (exact); successful reads with a wrong key = 0; data loss after restoring the key = 0.

---

### NFT-SEC-24: Master-key rotation and range reads [v1.0]

**Traces to**: AC-43, AC-2; RESTRICT-08 | **Data**: one 1 GiB blob, 10,000 blobs | **Status**: READY (RR Encryption #2, #3)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Record SHA-256 of blob files; rotate the master key with `sunnycloud admin rewrap --new-key-file` | Blob file hashes unchanged |
| 2 | Read files with the new key; start with the old key | New key works; old key refused |
| 3 | `kill -9` the `admin rewrap` run midway; restart | Every blob readable with the old or the new key; rerun completes |
| 4 | 1,000 random 4 KiB ranges, plus offsets at chunk edges (64 KiB − 1, 64 KiB, spanning two chunks, last bytes) | Bytes equal source; latency p95 ≤ 5 ms |
| 5 | Range beyond EOF | 416 |

**Pass criteria**: blob bytes rewritten by rotation = 0; unreadable blobs at any moment = 0; range mismatches = 0; 4 KiB range latency p95 ≤ 5 ms (RR Thresholds #1).

---

### NFT-SEC-25: Tampered ciphertext on disk is never served as plaintext [v1.0]

**Traces to**: AC-43, AC-7, AC-24; RESTRICT-08 | **Data**: 3 copies of a 100 MiB blob | **Status**: READY (`b_server_storage`, STREAM final-flag and AAD design)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Flip one bit in chunk k of blob 1; read a range inside chunk k | Error; no bytes of chunk k delivered; other chunks still readable |
| 2 | Truncate blob 2 by one chunk; read the end | Error; client download fails the hash check |
| 3 | Swap the files of two blob IDs on disk | Both reads fail |
| 4 | Run `sunnycloud scrub run --full` | All three problems reported |

**Pass criteria**: altered plaintext bytes delivered = 0 (exact); problems reported by scrub = 3 of 3.

---

### NFT-SEC-26: Scrub detects a flipped bit and does not hurt users [v1.0]

**Traces to**: AC-24, AC-5; RESTRICT-07 | **Data**: 100 blobs, big-file | **Status**: READY (RR Auth #13, RR Thresholds #4: transfers keep ≥ 90% of baseline during the scrub)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Flip 1 bit in 1 of 100 blobs; delete the file of another blob; add an unreferenced file | Next scrub reports 1 mismatch, 1 missing, 1 untracked, each with identifiers |
| 2 | Compare intact blobs | False positives = 0 |
| 3 | Measure baseline throughput of a 5 GiB upload without a scrub, then repeat during a scrub; also a 5 MB edit on A observed on B | Throughput ≥ 90% of baseline; edit reaches B ≤ 5 s p95 |
| 4 | After the scrub | No blob modified or deleted by the scrub |

**Pass criteria**: reported mismatches = exactly the injected set (exact); throughput during the scrub ≥ 90% of baseline (exact threshold); edit propagation ≤ 5 s p95.

---

### NFT-SEC-27: Account deletion removes everything [v1.0]

**Traces to**: AC-27, AC-30, AC-21; RESTRICT-10 | **Data**: account with many-small, photos, clip-1080-sdr, 5 shares, 3 sessions, 2 devices | **Status**: READY (RR Deletion #5)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | User deletes the account | Sessions and devices rejected ≤ 5 s; share links answer 404 uniformly; all former IDs answer 404 |
| 2 | Poll until the deletion finishes, at most 15 min after the request; inspect blob volume, thumbnail and transcode stores | 0 files for the account's recorded blob IDs |
| 3 | Dump the database; search for the account ID, file names, share-token hashes, session hashes | 0 rows |
| 4 | Check blobs while retention is still 7 days | Already gone (no retention for account deletion) |
| 5 | Account B | Hash-set and counts unchanged |
| 6 | Same identity logs in again | Treated as an unknown identity; no data returns |

**Pass criteria**: remaining rows = 0 and blobs = 0 (exact) within 15 min of the request (RR Thresholds #9); B unchanged.

---

### NFT-SEC-28: Red-team media corpus ends in job failure without exposure [v1.0]

**Traces to**: AC-25, AC-10; RESTRICT-13 | **Data**: redteam-media | **Status**: READY (RR Auth #12, RR Thresholds #5: photos 2 CPU / 2 GiB / 120 s; video 4 CPU / 4 GiB / 40 min)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Upload SVG (with `file:///etc/passwd` reference and external entity) and PDF renamed `.png`, an HLS playlist referencing `/etc/passwd` as `.mp4`, a 30000×30000 PNG, a 20 GB sparse video, a truncated HEIC | Upload accepted; originals stay downloadable as attachments |
| 2 | Observe jobs (test run uses short mock timeouts; a config read asserts the defaults 120 s for photo jobs and 40 min for video jobs) | Each ends `failed` within its configured time limit + 10 s |
| 3 | Sample decoder container memory and CPU every 1 s | Photo jobs ≤ 2 GiB and 2 CPUs; video jobs ≤ 4 GiB and 4 CPUs |
| 4 | Search all outputs and job messages for `root:x:0:0` and the planted canary content | 0 matches |
| 5 | Run 10 hostile uploads concurrently, plus normal 12 MP uploads | Normal thumbnails p95 ≤ 5 s; workers stay healthy |
| 6 | After 60 s | No leftover ffmpeg or vips processes; scratch files = 0 |

**Pass criteria**: jobs not ending in failure = 0 of 6; exposed bytes = 0; stuck jobs = 0; limit overruns = 0.

---

### NFT-SEC-29: Decoder and supervisor isolation [v1.0]

**Traces to**: AC-25; RESTRICT-13, RESTRICT-28 | **Data**: deployed worker, redteam-media | **Status**: READY (restrictions); items marked [research] come from `b_security` F1

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Inspect both decoder containers (interactive and general) | Each has `network_mode: none` (loopback only; the socket volume is the only channel); env has none of the database URL or password, master key, setup token, provider secrets; 0 mounts of the blob store |
| 2 | Inspect limits on both decoders | Photo jobs 2 CPU / 2 GiB, video jobs 4 CPU / 4 GiB (AC-25); [research F1] non-root, `cap_drop ALL`, no-new-privileges, read-only root, seccomp, pids limit |
| 3 | From inside each decoder, try TCP to Postgres and the API, an external IP, DNS, the metadata address, the blob path | 6 of 6 fail |
| 4 | Inspect supervisor | Has no database credentials (no database URL, role or secret); reaches only the API on the worker listener `:8081`; no blob mount; no master key; `mem_limit` set |
| 5 | From the supervisor try to connect to Postgres and to read accounts, nodes, shares, sessions through any path other than the job routes of `:8081` | No route to the database; other routes answer 403 on the internal listener and 404 on the public one |
| 6 | Upload an HLS playlist, `concat:`, `tcp:`, `subfile:` and `data:` inputs pointing to the canary | Canary hits = 0 |
| 7 | Process list during a job | ffmpeg has protocol and format whitelists; `VIPS_BLOCK_UNTRUSTED=1` set; HEIC uses its own profile |

**Pass criteria**: exposures = 0; successful network or mount access from the decoder = 0; canary hits = 0.

---

### NFT-SEC-30: SSRF through provider discovery and metadata [v1.0]

**Traces to**: AC-56, AC-19; RESTRICT-17, RESTRICT-20, new restrictions bullet (provider timeout 10 s) | **Data**: oauth-mock acting as a hostile provider | **Status**: READY (rule agreed with the coordinator: outbound provider requests go only to the operator-configured issuer URLs and the endpoints they advertise, redirects are never followed, link-local and cloud-metadata destinations are never contacted). Operators with a LAN identity provider must keep working.

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Discovery document advertises `token_endpoint`, `jwks_uri`, `userinfo_endpoint` at `169.254.169.254`, another link-local address, `[fe80::1]`, the cloud-metadata hostname, and decimal/hex forms of these | Sign-in fails with a generic error; canary hits = 0 |
| 2 | An endpoint on the configured issuer answers 302 to the canary and to a metadata address | Redirect not followed; canary hits = 0 |
| 3 | Hostname that resolves to a public address first and to `169.254.169.254` at connect time (rebinding) | Connection blocked on the resolved address |
| 4 | A callback or API request carries an issuer, `iss` or discovery URL chosen by the client | No outbound request to it; 0 flows to hosts that are not configured issuers |
| 5 | `file:///etc/passwd` and other non-HTTPS schemes advertised | Rejected |
| 6 | Positive control: configured issuer on a private LAN address (10.x) | Sign-in works |
| 7 | Provider endpoint stalls for 60 s | Request aborted at the 10 s timeout (+1 s margin) |

**Pass criteria**: outbound flows to link-local, metadata or non-configured hosts = 0 (exact); redirects followed = 0; accounts created = 0; LAN issuer sign-in succeeds; server answers a normal request afterward.
---

### NFT-SEC-31: Hostile-name corpus on the desktop [v1.0 Linux and Windows, v1.1 macOS]

**Traces to**: AC-37, AC-42; RESTRICT-06, RESTRICT-24 | **Data**: tricky-names (40 cases per OS) | **Status**: READY (RR Desktop #13, RR Thresholds #11); the exact list of 40 cases per OS is a test-data item (G-S10)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 0 | The real server is asked to create `/`-containing, `.`, `..`, NUL, control and bidi-control names | Rejected by the server (AC-37 policy); other names accepted |
| 1 | A hostile test server (a compromised server is in the threat model) sends 40 hostile entries: traversal (`../`, `..\`, absolute, UNC, drive), alternate data streams, reserved names and trailing dot/space, bidi controls, symlink in path, junction (Windows), case and NFC/NFD collisions, NUL and control characters, `.` and `..` segments, 300-character path | Names the OS cannot represent (reserved names, trailing dot or space, `:` on Windows, case or NFC/NFD collisions) are saved under a `%XX`-escaped name and reported; traversal and bidi entries refused; a named sync error per such entry; daemon keeps running |
| 2 | Tree-hash everything outside the root (canary directory, home, startup folder, `%APPDATA%`) | Unchanged |
| 3 | A local process swaps a directory and a symlink 1,000 times while the server sends 1,000 files | No write outside the root |
| 4 | Plant a symlink and a junction in the root pointing to a canary directory with secrets | 0 bytes uploaded from the target; skip logged |
| 5 | Compare pre-existing in-root files | None overwritten |

**Pass criteria**: writes outside the sync root = 0; overwritten files = 0 (tree hash); uploaded bytes through links = 0; daemon crashes = 0.

---

### NFT-SEC-32: Daemon IPC is reachable only by its own user [v1.0 Linux and Windows, v1.1 macOS]

**Traces to**: AC-48; RESTRICT-06 | **Data**: daemon IPC | **Status**: READY (RR Desktop #14)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Linux: `eve` finds and connects to the socket | Refused; socket 0600 inside a 0700 runtime directory owned by the daemon user |
| 2 | Windows: another local user opens the pipe; a remote client connects over SMB | Access denied / refused |
| 3 | Another user creates the pipe name before the daemon starts | Daemon does not use it and reports an error; settings window does not connect to it |
| 4 | Read the published IPC schema | 0 methods with token, secret or credential results |
| 5 | Call every method as the owning user and search responses and log-tail output for a known token | 0 matches |

**Pass criteria**: connections by other users or remote clients = 0; token-returning methods = 0.

---

### NFT-SEC-33: Updater rejects bad builds and ignores other sources [v1.0 Linux and Windows, v1.1 macOS]

**Traces to**: AC-49; RESTRICT-30 | **Data**: update host serving test builds | **Status**: READY (RR Desktop #15)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Offer an unsigned update manifest; a manifest signed with a key that is neither of the two pinned keys; a valid signature with a lower version; a same-version replay; an expired manifest (`expires` in the past); a manifest whose `serial` is not above the last accepted one; a payload altered after signing (SHA-256 does not match the signed manifest); a truncated download | All 8 rejected; installed binary hash unchanged; error logged |
| 2 | The sync server answers update-manifest requests | Daemon never asks it; requests to the sync server for updates = 0 |
| 3 | Set an update URL by environment variable and by editing the settings file | Ignored; the source stays the one fixed at build time; requests to the override host = 0 |
| 4 | Offer a correctly signed higher version, signed with the active key (positive control), then another signed with the recovery key | Both installed |

**Pass criteria**: rejected builds installed = 0 of 8; updates fetched from a non-build-time source = 0.

---

### NFT-SEC-34: Device-token storage on the desktop [v1.0]

**Traces to**: AC-48, AC-21; RESTRICT-19 | **Data**: Linux with and without Secret Service, Windows | **Status**: READY (restriction text)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Windows and Linux with a keyring: sign in the daemon | Token stored in the OS credential store; 0 occurrences in files under the user profile |
| 2 | Linux without a keyring | Token in a user-only file (mode 0600, directory 0700); one warning line in the log |
| 3 | `eve` reads that file | Denied |
| 4 | Search process arguments, environment, logs | 0 token occurrences |

**Pass criteria**: token in readable-by-others locations = 0; fallback warning logged exactly when the file path is used.

---

### NFT-SEC-35: Advisory gates in CI [v1.0]

**Traces to**: AC-63; RESTRICT-12 | **Data**: CI pipeline, branch with changes | **Status**: READY (RR Auth #14); the test uses the project's documented ignore mechanism and dates

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Add a dependency with a known RustSec advisory, no ignore | `cargo audit` and `cargo deny` jobs fail |
| 2 | Add an ignore without reason or date | Fails |
| 3 | Add an ignore with reason, dated 91 days ago | Fails |
| 4 | Same ignore dated 89 days ago | Passes |
| 5 | Pin tokio 1.54.0 (yanked), h2 below 0.4.16, bytes below 1.11.1, rustls below 0.23.45 | Each fails |
| 6 | Read branch protection | Both jobs are required checks |

**Pass criteria**: jobs green on a bad change = 0; jobs red on a valid dated ignore = 0.

---

### NFT-SEC-36: Release images are signed, listed and clean [v1.0]

**Traces to**: AC-63; RESTRICT-12, RESTRICT-13, RESTRICT-29 | **Data**: release images | **Status**: READY (RR Auth #15)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Verify the signature of each release image | Passes; an unsigned or rebuilt image fails |
| 2 | Fetch the SBOM | Valid; lists ffmpeg, libheif, libvips, libwebp with versions; libheif ≥ 1.22.0 |
| 3 | Scan the worker image | HIGH/CRITICAL findings in ffmpeg, libheif, libvips, libwebp = 0 |
| 4 | Image age and scheduled rebuild runs | Age ≤ 7 days at release; a successful scheduled rebuild in each 7-day window of the last 28 days |
| 5 | `ffmpeg -version` configuration | No `--enable-nonfree`, no `libfdk_aac` |
| 6 | Compose file image references | All by digest |

**Pass criteria**: counts as above (exact).

---

### NFT-SEC-37: CI pinning and workflow hygiene [v1.0]

**Traces to**: AC-63; RESTRICT-30, RESTRICT-12 | **Data**: workflows, Dockerfiles | **Status**: READY (`b_security` F34)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Lint all workflows | Every `uses:` pinned to a 40-hex commit; 0 tags |
| 2 | Add a workflow using `actions/checkout@v4` | Lint fails |
| 3 | Check default permissions and `pull_request_target` | Read-only default; no fork checkout under `pull_request_target` |
| 4 | Check `FROM` lines and release compose | Digests only |

**Pass criteria**: unpinned references = 0; permissive defaults = 0.

---

### NFT-SEC-38: No secrets in git, images or logs [v1.0]

**Traces to**: security_approach (Secrets), AC-23, AC-12; RESTRICT-18, RESTRICT-19 | **Data**: repository history, images, logs after a full suite run | **Status**: READY

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Secret-scan the full git history and image layers | 0 findings |
| 2 | Run the whole functional suite; grep API, worker, Caddy and Postgres logs for issued access, refresh and device tokens, setup and invite tokens, share tokens, OAuth `code` and `state`, master key (hex and base64), provider secrets | 0 hits |
| 3 | Send 20 malformed requests | No stack trace, SQL text or file path in any response body |
| 4 | Call `POST /test/clock` (`:9091`) and `GET /internal/v1/test/clock-offset` (`:8081`) on the release image | HTTP 404 each (the control endpoints exist only in the test image) |

**Pass criteria**: secret hits = 0 in each source; internal detail leaks = 0; `/test/clock` and `/internal/v1/test/clock-offset` on the release image answer 404 (exact).

---

### NFT-SEC-39: Vault server sees only ciphertext; tampering is rejected [v1.2]

**Traces to**: AC-13; RESTRICT-22, RESTRICT-23 | **Data**: vault-file | **Status**: READY (RR Vault #1 to #3)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Upload a 100 MiB file to a vault; fetch the stored bytes | Differ from plaintext; marker matches = 0 |
| 2 | Download and decrypt with the right passphrase | Hash equals original |
| 3 | Wrong passphrase | Clear error; no output file |
| 4 | Acting as a malicious server: flip one bit, truncate by a chunk, reorder two chunks, insert a chunk of another file or vault | Each rejected with a clear error; no output file, no partial plaintext left |

**Pass criteria**: tampered variants accepted = 0 of 4 on web, desktop and Android; plaintext at rest in server storage = 0.

---

### NFT-SEC-40: Wrapped-key and ciphertext swaps [v1.2]

**Traces to**: AC-60; RESTRICT-23 | **Data**: vault-file | **Status**: READY (RR Vault #5)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | 1,000 randomised swaps of a wrapped key plus ciphertext pair to another file or another vault | All rejected |
| 1b | Move a vault file to another folder of the same vault | Still opens (the parent is not bound) |
| 2 | Swap only the wrapped key; only the ciphertext | Rejected |
| 3 | Empty file; files of 1, 2 and 16 × chunk size; chunk size ± 1 byte | Round-trip with equal hash |
| 4 | Unknown format versions (0, 2, 255) | Rejected |

**Pass criteria**: swaps rejected = 1,000 of 1,000 (exact); round-trip failures = 0; unknown versions accepted = 0.

---

### NFT-SEC-41: Header fuzzing and KDF downgrade [v1.2]

**Traces to**: AC-61, AC-13; RESTRICT-23, new restrictions bullet (vault passphrase) | **Data**: vault-fuzz (10,000 mutations) | **Status**: READY (RR Vault #6; restrictions: at least 12 characters and a strength check)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Open 10,000 mutated headers (bit flips at every offset, truncations, extensions) | None accepted |
| 2 | Memory 63 MiB and 513 MiB, 64 GiB | Rejected; 64 MiB and 512 MiB accepted |
| 3 | Passes 2 (rejected) and 3 (accepted); parallelism 2; salt 15 and 17 bytes | Out-of-bounds rejected |
| 4 | Chunk size 64 KiB − 1 and 4 MiB + 1 | Rejected; 64 KiB and 4 MiB accepted |
| 5 | Server returns weaker parameters than the vault was created with | Rejected; no fallback path |
| 6 | Create a vault with an 11-character passphrase; a 12-character passphrase with zxcvbn score 3; a 12-character passphrase with zxcvbn score 4; a passphrase produced by `generate_passphrase` | First two refused; the last two accepted (rule of `restrictions.md`: at least 12 characters and zxcvbn score 4, or generated; RR Second-round #6 still says score 2 and 3 and is stale) |

**Pass criteria**: accepted mutations = 0 of 10,000 (exact); weak or short passphrases accepted = 0; peak client memory when rejecting the 64 GiB header ≤ peak memory of a normal unlock.

---

### NFT-SEC-42: Web vault page: CSP, third parties, heap, fragment [v1.2]

**Traces to**: AC-62, AC-13; RESTRICT-22, RESTRICT-23 | **Data**: web vault page, share link with key in the fragment | **Status**: READY (RR Vault #7); the external review is a manual release gate

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Parse the CSP of the vault page | No `unsafe-inline`, no `unsafe-eval`; `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'` |
| 2 | Load the page with network logging | 100% of requests to the own origin; third-party hosts = 0 |
| 3 | Unlock, open files, lock; snapshot the heap and worker/WASM memory; search for the vault key and passphrase | 0 matches |
| 4 | Inspect local/session storage, IndexedDB, cookies | No vault key or passphrase |
| 5 | Removed by owner decision 2026-11-05: no vault share links in v1.2 | — |
| 6 | Release gate | Signed external review report present (manual) |

**Pass criteria**: all counts 0 (exact); CSP tokens absent; review sign-off recorded.

---

### NFT-SEC-43: Vault limits and metadata exposure [v1.2]

**Traces to**: AC-14, AC-13, AC-46; RESTRICT-23 | **Data**: vault-file 100 MiB and 101 MiB, Android camera backup | **Status**: READY (RR Vault #4)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Upload 104,857,600 bytes | Accepted |
| 2 | Upload 101 MiB | Rejected at the start with a message naming the limit; bytes sent ≤ 2 MiB |
| 3 | Raise the configured limit to 200 MiB | 101 MiB accepted |
| 4 | Take a photo with camera backup on | Uploaded to a normal folder; vault entry count unchanged |
| 5 | Search the database and API responses for a vault file name marker | 0 matches; only sizes and ciphertext IDs visible |
| 6 | Thumbnail and transcode jobs for vault files | Jobs = 0; thumbnail endpoint 404 |
| 7 | Unlock a vault (key derivation) on a native client and in the browser | ≤ 1 s native, ≤ 2 s browser (RR Thresholds #6) |

**Pass criteria**: unlock time ≤ 1 s native and ≤ 2 s browser; 100 MiB accepted, 101 MiB rejected (exact); plaintext names on the server = 0; camera-backup files in the vault = 0.

---

### NFT-SEC-44: Android session token is excluded from backups [v1.0 Android, v1.1 iOS keychain check]

**Traces to**: AC-21; RESTRICT-19, new restrictions bullet (Android token excluded from backups) | **Data**: Android emulator (userdebug) and a physical Pixel | **Status**: READY (restrictions); iOS counterpart (`ThisDeviceOnly` keychain access) is [v1.1]

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Sign in, then run a full app backup (`bmgr` / `adb backup` / cloud backup transport) and unpack it | 0 files containing the access or refresh token of the mobile `sessions` row (search known token values) |
| 2 | Restore that backup on a second emulator | App is signed out; no session exists on the server for the new device |
| 3 | Search app logs and shared preferences in the backup | 0 token occurrences |

**Pass criteria**: tokens present in backup data = 0; sessions usable after restore = 0.

---

### NFT-SEC-45: A compromised decoder cannot outlive its job, read the next job or attach to the runner [v1.0]

**Traces to**: AC-25; RESTRICT-13, RESTRICT-28 | **Data**: redteam-media, deployed worker, a test hook that runs scripted commands as the decoder user | **Status**: READY (Step 4 decoder hardening decision)

**Fault injection**: `e2e-ctl exec-in-decoder` (test stack only) runs a script as the decoder user, as a child of the runner during a job, to stand in for code execution inside the decoder.

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | During job J1 the script starts a child with `setsid` (new session, detached) that sleeps for 10 minutes, and a second child that double-forks | After J1 ends, every descendant is gone (SIGKILL by the runner, which is the child subreaper): process list of the decoder container shows only the runner, within 1 s |
| 2 | The leftover-seeking child from step 1 tries to read the scratch directory, the socket volume and `/proc/*/fd` for the input of the next job J2 (uploaded with a canary) | 0 bytes of J2 readable; canary hits = 0; scratch is wiped between jobs |
| 3 | The script runs `ptrace`/`process_vm_readv` against the runner and reads `/proc/<runner>/mem` | All denied (seccomp denies `ptrace` and `process_vm_*`; the runner is not dumpable) |
| 4 | Make the script's job end in a hostile-class outcome (killed by the watchdog, memory limit or format rejection) | The runner exits after the outcome; Compose restarts the container; a fresh runner serves J2 |
| 5 | Inspect the decoder container | `network_mode: none` (loopback only), read-only root, non-root user, `cap_drop ALL`, no-new-privileges |

**Pass criteria**: surviving descendants after a job = 0 (exact); bytes of the next job readable = 0; successful ptrace or cross-process memory reads = 0; runner restarts after a hostile outcome = 1 of 1.

---

### NFT-SEC-46: Database roles: the API cannot change the schema, workers have no database access [v1.0]

**Traces to**: AC-25, AC-65; RESTRICT-13 | **Data**: deployed stack, read-only DB probe actions of `e2e-ctl` | **Status**: READY (Step 4 S4-1, S4-2)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | `e2e-ctl` probe of the role catalog | Roles `sunnycloud_migrator` and `sunnycloud` exist; no role `sunnycloud_worker`; `sunnycloud` is not superuser, has no `BYPASSRLS`, owns no table and has no `CREATE` on the schema; the migrator owns the schema |
| 2 | Probe as `sunnycloud`: `CREATE TABLE`, `ALTER TABLE nodes ...`, `DROP TABLE`, `TRUNCATE` | Permission denied for each |
| 3 | `docker inspect` and secret mounts of every container | `db_migrator_url` is mounted only in the `migrate` service; the worker and the decoders have no database URL, password or secret in env or mounts |
| 4 | From the worker container try to connect to `db:5432` | No route (the worker reaches only the API on `:8081`); 0 successful connections |
| 5 | Check the networks | `caddy` is on `proxy` only; `:8081` and `:9090` of the API answer from `backend` and not from `proxy` |
| 6 | Start the API against a database one migration behind | The API exits with a schema-version message and serves nothing; the schema is unchanged (the API did not migrate); after `migrate` runs it starts |

**Pass criteria**: DDL statements accepted for `sunnycloud` = 0 (exact); database logins held by the worker = 0; connections from the worker to Postgres = 0; API starts on a mismatching schema = 0.

---

### NFT-SEC-47: The audit log is append-only for the API role [v1.0]

**Traces to**: AC-21, AC-30 (audited events); design rule of `common-helpers/04_helper_audit.md` | **Data**: deployed stack with sign-in, revocation and admin events | **Status**: READY (Step 4 S4-2)

| Step | Consumer Action | Expected Response |
|------|----------------|------------------|
| 1 | Probe as `sunnycloud`: `INSERT` and `SELECT` on `audit_log` | Both succeed (events from sign-in, device revocation and an admin action are present) |
| 2 | Probe as `sunnycloud`: `UPDATE`, `DELETE`, `TRUNCATE` on `audit_log`, `GRANT` and `ALTER TABLE` | Each is denied (the role neither owns the table nor holds those privileges) |
| 3 | Advance the clock past the audit retention and let the janitor run the purge | Rows older than the retention are removed through the `SECURITY DEFINER` purge function owned by the migrator; newer rows remain |
| 4 | Run any API operation that tries a different statement against the table (CI statement check) | No statement other than INSERT, SELECT and the purge function call reaches the table |

**Pass criteria**: accepted UPDATE, DELETE, TRUNCATE or GRANT by the API role = 0 (exact); rows newer than the retention removed by the purge = 0; audit events missing for the three exercised actions = 0.

---

### NFT-SEC-48: Two concurrent redemptions of one invite create exactly one account [v1.0]

**Traces to**: AC-75, AC-23; RESTRICT-18 | **Data**: oauth-mock (DS-20, scenario `valid`, a fresh verified provider identity for every redemption), invite tokens | **Env**: C, group P (every trial uses its own invite and identities) | **Status**: READY (AC-75 text)

**Concurrency**: 200 trials of 2 concurrent redemptions, 1 trial of 50 concurrent redemptions, 1 trial of the same identity twice. Redemption happens at the provider callback after `POST /api/v1/auth/login-attempts` carried the invite token; all callbacks of one trial are released from a barrier (the two-way trials use `e2e-consumer` and `e2e-consumer-b`).

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Create the invite with `sunnycloud admin invite` (member invite) for each trial | Token returned once |
| 2 | 2-way trial: two different mock identities complete the callback at the same instant with the same invite | One callback ends with a session for a new account; the other ends with a clear invite error (already redeemed), no session |
| 3 | 50-way trial: 50 identities, one invite | 1 account, 49 invite errors |
| 4 | Same identity twice with one invite (double submit) | 1 account; every session issued belongs to that account |
| 5 | After each trial: the admin user list and ctl read-only counts of `accounts` and `identities` | Accounts +1; identities +1 (+0 for each losing identity: no partial account) |
| 6 | Redeem the used invite a third time, sequentially | Rejected |

**Pass criteria**: accounts created per invite = 1 in 202 of 202 trials (exact); sessions issued to losing identities = 0; identity or account rows left by a losing redemption = 0; responses of 5xx = 0.

---

### NFT-SEC-49: A device code exchanges for a token exactly once and never after it expired [v1.0]

**Traces to**: AC-76, AC-57; RESTRICT-19 | **Data**: device grant, an approving session | **Env**: C, group X (config `device_code_ttl` 3 s and `device_poll_interval` 50 ms for the race trials; the production maximum of 10 min is asserted by reading the config; T-CLOCK for step 4) | **Status**: READY (AC-76 text)

**Concurrency**: 8 concurrent polls of one device code per grant.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | 200 grants: `POST /api/v1/device-grants`, approve from a session, then 8 concurrent `POST /api/v1/device-tokens` with the same `device_code` | Exactly 1 of the 8 answers 200 with a token; the other 7 answer non-2xx and carry no token field |
| 2 | For each grant poll once more after a pause, and use the token once on a sync call | The late poll fails; the token works (200) |
| 3 | 50 grants with the short TTL: approve at a seeded 0 to 200 ms before expiry, then 8 concurrent polls spanning the expiry instant; ctl read-only probe of `device_tokens.created_at` and the grant's `expires_at` (both from the injectable clock) | No token has `created_at` later than the grant's `expires_at`; grants polled only after expiry have 0 tokens |
| 4 | With T-CLOCK: approve a fresh grant, advance the clock past its expiry, poll | Rejected as expired; 0 tokens minted |

**Pass criteria**: tokens minted per approved-in-time code = 1 (exact, 200 of 200); tokens minted for one code more than once = 0; tokens minted after expiry = 0; late or repeated exchanges that succeed = 0.

---

### NFT-SEC-50: Worker endpoints reject user sessions and device tokens (403) and do not exist on the public route (404) [v1.0]

**Traces to**: AC-79; RESTRICT-13, RESTRICT-16 | **Data**: a user session, a device token, the worker secret (held by `e2e-ctl` only), 3 photos uploaded while the supervisor is paused (3 queued jobs) | **Env**: C, group X (`docker pause worker-supervisor` for the whole scenario so only the test holds the worker identity) | **Status**: READY (AC-79 text; needs the `e2e-ctl internal-http` action of `environment.md`)

The 8 worker operations are the routes of component 08: `POST /api/v1/worker-jobs/claims`, `PUT` and `DELETE /api/v1/worker-jobs/{job_id}/lease`, `GET .../input`, `PUT .../outputs/{kind}`, `PUT .../result`, `POST .../failure`, `PUT /api/v1/worker-jobs/status`.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | From the backend network (through `internal-http` to `api:8081`) call each of the 8 operations with: a user session bearer, a user session cookie, a device token, no `Authorization`, a wrong worker secret (40 requests) | 403 for all 40, bodies equal after removing the request id (no cause can be told apart); the 3 jobs stay queued and unclaimed (`admin media-stats` unchanged) |
| 2 | Positive control: the real worker bearer on `claims`, then `DELETE` of the lease it received | 200 with a claimed job, then 204 on release; the job is queued again, so the routes do exist on `:8081` |
| 3 | From the consumer, through `https://sunnycloud.test`, call the 8 operations with a user session, a device token and no credential (24 requests) | 404 for all 24 |
| 4 | Through `internal-http` aimed at the public URL, call the 8 operations with the real worker bearer (8 requests) | 404 for all 8: the route is absent from the public router, not just protected |
| 5 | From the consumer and from the `caddy` container open TCP to `api:8081`; inspect published ports and the active Caddy config | No connection succeeds; `8081` is not published; the config has no handler that matches `/api/v1/worker-jobs` |

**Pass criteria**: 403 on 40 of 40 and 404 on 32 of 32 (exact); jobs claimed or changed by non-worker credentials = 0; successful TCP connections to `:8081` from the proxy or client networks = 0; the positive control succeeds (the test cannot pass by the routes simply not existing).

---

<!-- architecture-sync 2026-11-04 (greenfield Step 5): NFT-SEC-51 covers the public-share password state in ui_mockups/web-public-share.html (06 `open` returns `PasswordRequired` with no file fields). -->

### NFT-SEC-51: A password-protected link reveals nothing about its file before unlock [v1.0]

**Traces to**: AC-12 | **Data**: small-fixtures (DS-23), the 5 MB file uploaded as `secret-plan.pdf` and shared with a password | **Env**: C (API client and Playwright Chromium), group P | **Status**: READY (the response status is open, G-9; this test asserts content only)

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Open the link without a password through the API and through the public page | Response and DOM contain no file name, size, media type or expiry: 0 occurrences of `secret-plan` and of the byte size |
| 2 | Request the content with GET, HEAD and `Range: bytes=0-0` | No content bytes; no `Content-Disposition` naming the file |
| 3 | Enter a wrong password | Same result as step 1, plus the wrong-password message |
| 4 | Enter the right password | Name, size and media type shown; the download hash equals the file |

**Pass criteria**: metadata fields exposed before unlock 0 (exact); content bytes before unlock 0 (exact); unlock with the right password 1 of 1.

---

## Gaps

No scenario in this file is BLOCKED.

- **G-S5** (not blocking) TLS 1.2: RESTRICT-11 says TLS 1.3 but not that 1.2 must be refused, so NFT-SEC-11 asserts only TLS 1.3 negotiated and TLS 1.1 and older refused.
- **G-S6** (not blocking) NFT-SEC-13 step 4: the exact status for admin-only operations called by a non-admin is not specified; the test asserts 403 or 404, never 2xx.
- **G-S10** (test data) The 40 hostile-name cases per OS are named by class only in `data_parameters.md`; the exact list and count per class must be added to the test data (NFT-SEC-31).
- **G-S12** (no ID yet) `restriction-ids.md` has no IDs for the new restrictions bullets (derived blobs encrypted; leases and timeouts; Android token and vault passphrase; no-internet sessions). They are cited as "new restrictions bullet".
- **G-S13** (not an AC) Out of scope here: vault freshness against a malicious server is documented as a caveat, not tested; per-link download connection caps (`b_security` F19) are not an AC.

## AC coverage index

| AC | Tests |
|----|-------|
| AC-2 | NFT-SEC-15, -18, -23, -24 |
| AC-4 | NFT-SEC-20 |
| AC-5 | NFT-SEC-26 |
| AC-7 | NFT-SEC-14, -20, -21, -22, -25 |
| AC-10 | NFT-SEC-28 |
| AC-12 | NFT-SEC-06, -11, -16, -17, -18, -19, NFT-SEC-51 |
| AC-13 | NFT-SEC-39, -41, -42, -43 |
| AC-14 | NFT-SEC-43 |
| AC-19 | NFT-SEC-02, -30 |
| AC-20 | NFT-SEC-01, -04, -05, -06, -17 |
| AC-21 | NFT-SEC-07, -08, -10, -27, -34, -44, -47 |
| AC-23 | NFT-SEC-01, -02, -38, -48 |
| AC-24 | NFT-SEC-25, -26 |
| AC-25 | NFT-SEC-28, -29, -45, -46 |
| AC-27 | NFT-SEC-27 |
| AC-30 | NFT-SEC-27, -38, -47 |
| AC-37 | NFT-SEC-22, -31 |
| AC-38 | NFT-SEC-20 |
| AC-39 | NFT-SEC-20 |
| AC-42 | NFT-SEC-22, -31 |
| AC-43 | NFT-SEC-23, -24, -25 |
| AC-46 | NFT-SEC-43 |
| AC-47 | NFT-SEC-19 |
| AC-48 | NFT-SEC-32, -34 |
| AC-49 | NFT-SEC-33 |
| AC-54 | NFT-SEC-11, -12 |
| AC-55 | NFT-SEC-02 |
| AC-56 | NFT-SEC-03, -30 |
| AC-57 | NFT-SEC-05, -07, -49 |
| AC-58 | NFT-SEC-09, -10 |
| AC-59 | NFT-SEC-13, -14, -15 |
| AC-60 | NFT-SEC-40 |
| AC-61 | NFT-SEC-41 |
| AC-62 | NFT-SEC-42 |
| AC-63 | NFT-SEC-35, -36, -37 |
| AC-65 | NFT-SEC-46 |
| AC-75 | NFT-SEC-48 |
| AC-76 | NFT-SEC-49 |
| AC-79 | NFT-SEC-50 |

## Restriction coverage index

| Restriction | Tests |
|-------------|-------|
| RESTRICT-06 | NFT-SEC-31, -32 |
| RESTRICT-07 | NFT-SEC-13, -14, -22, -26 |
| RESTRICT-08 | NFT-SEC-23, -24, -25 |
| RESTRICT-09 | NFT-SEC-20 |
| RESTRICT-10 | NFT-SEC-27 |
| RESTRICT-11 | NFT-SEC-09, -11, -13, -14, -15, -16, -19, -20, -21 |
| RESTRICT-12 | NFT-SEC-15, -35, -36, -37 |
| RESTRICT-13 | NFT-SEC-28, -29, -36, -45, -46, -50 |
| RESTRICT-16 | NFT-SEC-11, -50 |
| RESTRICT-17 | NFT-SEC-02, -03, -04, -30 |
| RESTRICT-18 | NFT-SEC-01, -02, -38, -48 |
| RESTRICT-19 | NFT-SEC-05, -07, -08, -09, -10, -34, -38, -44, -49 |
| RESTRICT-20 | NFT-SEC-03, -30 |
| RESTRICT-22 | NFT-SEC-11, -12, -39, -42 |
| RESTRICT-23 | NFT-SEC-39, -40, -41, -42, -43 |
| RESTRICT-24 | NFT-SEC-31 |
| RESTRICT-28 | NFT-SEC-29, -45 |
| RESTRICT-29 | NFT-SEC-36 |
| RESTRICT-30 | NFT-SEC-33, -37 |
| New bullet: derived blobs encrypted (no ID yet) | NFT-SEC-23 |
| New bullet: provider timeout 10 s, HSTS one year (no ID yet) | NFT-SEC-11, -30 |
| New bullet: Android token excluded from backups; vault passphrase of 12+ characters with strength check (no ID yet) | NFT-SEC-41, -44 |
| RESTRICT-01 to -05, -14, -15, -21, -25 to -27, -31 | not in this file's area |
