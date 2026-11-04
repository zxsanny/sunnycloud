# Proposed Gap Decisions (awaiting owner confirmation)

The five scenario files list gaps (G-1.. in blackbox, G in performance/resource-limit, G-R/G-S in resilience/security, environment/test-data gaps). These are the ones that block or blur a pass/fail verdict, with a proposed default. On approval they are written into `acceptance_criteria.md`, `restrictions.md` and `results_report.md`, and the matching BLOCKED scenarios are unblocked.

| # | Gap | Proposed default |
|---|-----|------------------|
| 1 | AC-56 says "10 negative callbacks" but lists 12 cases | Say 12; all 12 must pass |
| 2 | Refresh-token reuse grace: 30 s (AC) vs 10 s (research) | 30 s |
| 3 | "Compliant" video (never re-encoded) | H.264 High profile or lower, long side at most 1920 and short side at most 1080 (orientation independent), at most 10 Mbit/s video, AAC audio, SDR (BT.709). Index at the front: served as is. Index at the end with compliant codecs: lossless remux (stream copy, index moved to the front), never a re-encode (decisions 2 and 3, 2026-11-04) |
| 4 | Hostile names: what the server accepts and what the 40 cases per OS expect | Server rejects `/`, NUL, `.` and `..`, control characters and bidi-control characters; accepts everything else, including Windows-reserved names and case variants. A client that cannot represent a name on its OS saves it under a `%XX`-escaped name (decision 9), reports it, and never writes outside the sync root or overwrites a file |
| 5 | AC-20 numeric limits | Device approval: 5 wrong attempts per approving session, 20 per account per hour, 50 per IP per hour. Share passwords: 10 wrong attempts per link per 10 min. Invalid share tokens: 100 per minute per IP. Failed sign-in steps: 10 per minute per IP |
| 6 | Job lease, retries, orphan sweep | Lease 5 min, 3 attempts; upload reaper expires sessions after 24 h; orphan blobs older than 48 h are swept |
| 7 | Timeouts and headers | Postgres outage recovery within 60 s of the database returning; provider call timeout 10 s; HSTS max-age one year; account deletion completes within 15 min of the request |
| 8 | After a database restore, files deleted after the backup point | Resync uploads local-only files and downloads server-only files; nothing is deleted except through the mass-delete guard |
| 9 | Are thumbnails and transcodes encrypted at rest? | Yes, every blob including derived ones |
| 10 | Missing thresholds | 4 KiB range read ≤ 5 ms p95; NVENC at least 1.3x faster than CPU on the 1080p clip; desktop initial hashing uses at most 25% of one core while the user is active; scrub keeps transfers at ≥ 90% of baseline; decoder limits 2 CPU / 2 GiB / 120 s for photos and 4 CPU / 4 GiB / 40 min for video; vault unlock ≤ 1 s native and ≤ 2 s in the browser |
| 11 | Inline-display allowlist | image/jpeg, png, gif, webp, avif, and video/mp4, video/webm; everything else (including PDF) is an attachment |
| 12 | Registration allowlist | Single-use invite tokens in v1.0; an optional config list of verified email addresses; email matched only at redemption |
| 13 | Edited photo to original link | Linked when the same asset ID on the device has a new content hash (Android MediaStore ID); otherwise treated as a new file |
| 14 | Reference hardware and workload definitions | REF-SRV: 8 physical cores, 32 GB RAM, NVMe, 1 GbE. AC-1 mix: 70% idle long-poll sessions, 20% small-file edits, 10% large transfers. "100 background uploads" = 100 concurrent 100 MiB uploads. AC-5 clock starts when the file is closed on device A and ends when device B has the complete verified file |
| 15 | Server clock control for the 7-, 90- and 91-day tests | The server reads time through an injectable clock; the test image exposes control endpoints that do not exist in release builds (see row 24) |
| 16 | Android token backup, vault passphrase minimum, vault freshness | Android token excluded from backups; vault passphrase of at least 12 characters with a strength check; vault freshness caveat documented |
| 17 | Batch-commit retries and interrupted upgrade | Batch commit is idempotent by client-supplied commit ID; an interrupted upgrade rolls back the migration transaction and refuses to start until re-run |

## Round 2: remaining blocked scenarios (proposed, awaiting owner confirmation)

After the first round of defaults, 5 scenarios are fully blocked and 2 are partly blocked. They are the only reason Phase 3 of the test-spec gate cannot pass. Every AC is already covered by at least one other test; RESTRICT-26 is the only restriction with no runnable test.

| # | Scenario | Gap | Proposed default |
|---|----------|-----|------------------|
| 18 | FT-P-75, RESTRICT-26 | macOS permission denied behavior [v1.1] | When macOS denies access to a folder, the tray and settings window show "access denied: allow access in System Settings" within 10 s, the other folders keep syncing, and syncing of that folder resumes within 10 s after permission is granted |
| 19 | FT-P-22 | HEIC/EXIF orientation tolerance | Thumbnail dimensions swap correctly for orientations 5-8, and the thumbnail matches a reference render with SSIM ≥ 0.95 |
| 20 | FT-P-30 | Samsung and Xiaomi limits | With battery exemption on and Wi-Fi: p95 ≤ 5 min; results are published per device in the docs. A run without the phones is `NOT-RUN(HW-DEVICE)` |
| 21 | FT-P-33 | Live Photo [v1.1] | Deferred to v1.1 with its fixture: a Live Photo is stored as one asset with a linked still and video; the scenario is written when v1.1 is planned |
| 22 | FT-P-60 | Include/exclude pattern grammar | gitignore-style patterns relative to the pair root: `*`, `**`, `?`, leading `/` anchors, trailing `/` for folders, `!` negation; an excluded path is never uploaded, downloaded or deleted |
| 23 | FT-N-30 | Existing sessions with no internet | Existing sessions and device tokens keep working; only new sign-ins need the provider |
| 24 | FT-N-35 | Clock endpoint | `POST /test/clock` with `{"advance_seconds": N}` (harness to API) and `GET /internal/v1/test/clock-offset` (supervisor reads the same offset) exist only in the `-test` image and return 404 in release builds |
| 25 | FT-N-37 | Passphrase strength | At least 12 characters and a zxcvbn score of at least 3 (superseded: `restrictions.md` now requires score 4 or a generated passphrase) |
| 26 | NFT-SEC-09 | State-changing request with a cookie but neither `Origin` nor `Sec-Fetch-Site` | Rejected with HTTP 403 |
| 27 | NFT-SEC-21 | Largest file a Firefox or Safari download is verified for | Hash-verified in the browser up to 2 GiB; above that the page says "not verified in the browser" |

## Round 3: owner decisions of 2026-11-04 that change test expectations (`component-decisions.md` wins)

| # | Decision | Test consequence |
|---|----------|------------------|
| 28 | 1 (AC-53) | Android limited access shows "N photos visible, tap to allow more"; no "of M" (FT-P-35, DS-21) |
| 29 | 2, 3 (media) | Compliance bound is orientation independent; a compliant clip with the index at the end is remuxed, not re-encoded (FT-P-25, NFT-PERF-17, DS-35). Video poster frame at 1 s, or 10% of the duration for clips under 10 s (FT-P-23) |
| 30 | 4 (restore) | `admin restore` into a deleted folder lands in "Restored" at the root (FT-P-18) |
| 31 | 5 (epoch) | `doctor` reports Fail (not a warning) for a missing epoch bump detected from the `.epoch` marker on the blob volume, and `serve` refuses to start (NFT-RES-07) |
| 32 | 6 (scrub) | Scrub summaries are audit events and metrics, no new table (FT-P-43) |
| 33 | 7 (mobile scope) | No Android file browser or download in v1.0 (FT-N-31) |
| 34 | 8 (updater) | Offline-signed update manifest (two pinned Ed25519 keys, version, per-OS SHA-256, serial, expiry) from GitHub Releases, source fixed at build time (NFT-SEC-33, DS-31) |
| 35 | 12, 13 (deployment) | One API process; Caddy to API over h2c; worker listener `:8081`; two decoder containers; DB roles `sunnycloud_migrator` and `sunnycloud` only, the worker has no database login (environment.md, NFT-SEC-46) |
| 36 | 15 (conflict copy) | Device label in the conflict-copy name is the committing device (FT-P-10); share links target files only (FT-N-31) |
| 37 | 17 (CLI names) | `admin keycheck-init`, `admin rewrap`, `admin media-requeue`, `doctor`, `key-check` as in `components/12_operator_cli/description.md` |
