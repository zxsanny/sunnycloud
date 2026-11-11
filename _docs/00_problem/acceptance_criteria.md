# Acceptance Criteria

Numeric targets are initial defaults refined by research (`_docs/00_research/00_ac_assessment.md` and the round-2 reports in `_docs/00_research/raw/b_*.md`). Each criterion carries the release that must meet it: **[v1.0]**, **[v1.1]**, **[v1.2]**. Releases: v1.0 = server, web, Android, Linux and Windows desktop, GitHub and generic-OIDC sign-in (Google through OIDC), CPU transcoding. v1.1 = iOS, macOS desktop, Sign in with Apple, NVIDIA GPU encoding. v1.2 = end-to-end encrypted vault after an external audit. Facebook sign-in is out of scope.

## Scale
- AC-1 [v1.0]: One installation supports at least 500 registered users and 100 concurrent syncing sessions, tested at an aggregate of 1 Gbps with a mix of 70% idle long-poll sessions, 20% small-file edits and 10% large transfers.

## Transfer
- AC-2 [v1.0]: A single large-file transfer reaches at least 90% of available link speed, up to 1 Gbps (SSD or NVMe on both ends, round-trip time of at most 20 ms, file of at least 1 GiB, reverse-proxy request buffering off), including with blob encryption at rest (AC-43). The reference server (REF-SRV) has 8 physical cores, 32 GB RAM, NVMe storage and a 1 GbE link.
- AC-3 [v1.0]: An interrupted upload resumes and re-sends at most 16 MiB (4 lanes x 2 MiB in flight plus one 8 MiB checkpoint interval). (Changed from 10 MiB on 2026-11-11 by the owner, AZ-3731, G-03-1.)
- AC-4 [v1.0]: Files up to 50 GiB can be uploaded and downloaded; finishing an upload takes constant time regardless of file size.
- AC-38 [v1.0]: A block is acknowledged only after it is flushed to disk. After `kill -9` of the server during 100 concurrent uploads, 0 committed files contain missing or zero-filled blocks.
- AC-39 [v1.0]: 100 upload sessions that each declare 50 GiB are refused beyond the per-account session cap and the quota, and free disk space never falls below the configured floor.
- AC-40 [v1.0]: 10,000 files of up to 1 MiB upload in at most 60 s at 20 ms round-trip time, using single-request upload for files up to 2 MiB and a batch commit.

## Sync
- AC-5 [v1.0]: From the moment a file is closed on device A until device B holds the complete verified file, at most 5 s at p95 for files up to 10 MB, also while 100 concurrent 100 MiB background uploads are running.
- AC-6 [v1.0]: Server time to answer a listing of a folder with 10,000 entries is at most 300 ms at p95. The API pages large listings.
- AC-7 [v1.0]: Every transferred file is hash-verified end to end, with 0 undetected corruptions in transit. Corruption at rest is detected and reported by a scheduled scrub (AC-24). Safari and Firefox verify the hash in the browser up to 2 GiB; above that the page says "not verified in the browser".
- AC-8 [v1.0]: The later commit by server order (not client clock) becomes the current version, and the stored file is never corrupted or a mix of both. When a client's change was based on a version that is no longer current, the overwritten content is kept beside it as "name (conflict copy)". When one device deletes a file that another edited, the edit wins and the file stays. No version history is kept beyond that.
- AC-41 [v1.0]: After the database is restored from an older backup, every device receives `resync_required` and converges without missing changes. A device offline longer than the tombstone horizon (90 days) does a safe full resync. After a restore, local-only files are uploaded, server-only files are downloaded, and nothing is deleted except through the mass-delete guard.
- AC-42 [v1.0]: Names are unique per folder case-insensitively and are stored case-preserving; two names differing only by case never overwrite each other.

## Deletion safety
- AC-29 [v1.0]: If the sync root is missing, unmounted or empty, or 200 or more deletions arrive in one batch, the desktop client pauses deletes and asks the user. 0 deletions propagate until confirmed.
- AC-30 [v1.0]: A blob unreferenced by a delete or overwrite is not removed before the retention period (default 7 days, configurable 0 to 90). An operator command restores a file by path and time. Account deletion (AC-27) is immediate. The server reads time through an injectable clock so retention and tombstone tests can advance days; the control endpoint `POST /test/clock` with `{"advance_seconds": N}` exists only in the test image and returns 404 in release builds.

## Photos and videos
- AC-9 [v1.0 Android, v1.1 iOS]: On Android, with battery optimization exempted and on Wi-Fi, a new photo uploads within 60 s of capture on a Pixel; Samsung and Xiaomi p95 is at most 5 min with battery exemption on and Wi-Fi, published per device in the docs; a run without the phones is NOT-RUN(HW-DEVICE). On iOS, camera backup is best effort with no time guarantee; foreground catch-up completes the backlog.
- AC-10 [v1.0]: For non-bulk uploads, a thumbnail for a 12 MP photo is ready within 5 s of upload completing, at p95, using a reserved worker lane. For HEIC and EXIF-rotated photos the thumbnail dimensions swap correctly for orientations 5 to 8 and match a reference render with SSIM at least 0.95. A bulk import of 10,000 12 MP JPEGs finishes thumbnails in at most 15 min on the 4-core reference while that lane keeps p95 under 5 s.
- AC-11 [v1.0]: Transcode workers run on the operator's own machines. On a CPU-only reference worker with 4 physical cores: a 10-minute 1080p 30 fps SDR clip transcodes in at most 5 min; a 10-minute 4K HDR (HLG) clip, tone-mapped to 1080p SDR, in at most 20 min; a clip that is already compliant is never re-encoded (compliant = H.264 High profile or lower, within the orientation-independent bound of long side at most 1920 and short side at most 1080, at most 10 Mbit/s video, AAC audio, SDR BT.709; a compliant clip with the index at the end is losslessly remuxed with the index moved to the front instead of re-encoded). Adding workers increases throughput.
- AC-28 [v1.1]: A worker detects an NVIDIA NVENC encoder by a test encode at start and uses it only when the operator enables it (off by default); on the 1080p reference clip it is at least 1.3 times faster than CPU. If the GPU is missing or a job fails, the job falls back to CPU and the result is valid. Intel/AMD and Apple encoders are documented as experimental only.
- AC-44 [v1.0]: On a corpus of 100 mixed phone clips, derived storage (thumbnails plus transcodes) is at most 20% of original bytes.
- AC-45 [v1.0]: A stalled ffmpeg is killed within 90 s of its last progress line and retried once on CPU. Rotated and variable-frame-rate clips come out with correct orientation and audio drift of at most 40 ms.
- AC-46 [v1.0]: Re-installing the Android app on a phone with 5,000 already-backed-up assets uploads 0 bytes of media; an edited photo and its original produce linked, not duplicate, entries. Live Photo pairing is [v1.1]. An edited photo is linked to its original when the same Android MediaStore asset ID has a new content hash; otherwise it is a new file.

## Sharing and content serving
- AC-12 [v1.0]: A share link opens in at most 2 s, expires after 7 days by default (configurable), and revocation takes effect within 5 s. A password can be set optionally. Tokens carry at least 128 bits of randomness and are stored only as hashes; missing, expired and revoked links answer identically (status, body, headers, and median timing within 1 ms).
- AC-47 [v1.0]: Inline display is allowed only for image/jpeg, image/png, image/gif, image/webp, image/avif, video/mp4 and video/webm, served with `nosniff`. Every other type (HTML, SVG, XML, JS, PDF, polyglots) is served as an attachment with `nosniff` and a sandbox CSP; the automated test shows 0 inline renders of other types.

## Encryption at rest
- AC-43 [v1.0]: Blobs are encrypted on disk by the server with per-blob keys wrapped by an operator-supplied master key. A stolen disk without the master key yields no plaintext. Key rotation rewraps keys without rewriting blobs. Range reads and downloads meet AC-2. Losing the master key makes data unrecoverable, and the install guide says so and describes its backup. A 4 KiB range read takes at most 5 ms at p95. Thumbnails and transcodes are encrypted like every other blob.

## Vault (end-to-end encryption)
- AC-13 [v1.2]: The server stores only ciphertext for vault files. A vault file downloaded from the server differs byte for byte from the plaintext and decrypts correctly on the client. Tampered, truncated, reordered or wrong-vault ciphertext is rejected by the client.
- AC-14 [v1.2]: Vault files up to 100 MiB (configurable) are supported; larger ones are rejected with a clear message. Camera backup does not write into the vault. Vault unlock (key derivation) takes at most 1 s native and 2 s in the browser. Opening and decrypting a 100 MiB file peaks at no more than 250 MB total on iOS Safari (iPhone 12 or newer) and Chrome on a 4 GB Android phone.
- AC-60 [v1.2]: Moving a wrapped key and its ciphertext to another file or another vault is rejected in 1,000 of 1,000 randomised swaps, while moving a file to another folder of the same vault leaves it decryptable (the parent folder is not bound); empty files and exact multiples of the chunk size round-trip; unknown format versions are rejected.
- AC-61 [v1.2]: The client rejects vault headers with memory below 64 MiB or above 512 MiB, passes below 3, parallelism other than 1, chunk size outside 64 KiB to 4 MiB, or any altered header byte (0 accepted in 10,000 fuzzed mutations).
- AC-62 [v1.2]: The web vault page serves a CSP without `unsafe-inline` or `unsafe-eval`, loads 0 third-party hosts, and after locking a heap scan finds no vault key. An external review of the construction is passed before release.

## Desktop client
- AC-15 [v1.0]: With 100,000 watched files, the daemon uses at most 1% CPU when idle and at most 100 MB memory; the first scan (enumerate and stat) finishes within 60 s; initial hashing runs in the background at a throttled rate. The settings window is a separate process and is not counted. Initial hashing uses at most 25% of one core while the user is active.
- AC-16 [v1.0]: The settings window opens within 1 s (cold start at p95 on a 2019 Windows laptop with an HDD-class disk and on Ubuntu LTS Wayland with NVIDIA, with no environment-variable workarounds). A changed setting takes effect in the daemon within 5 s without a restart.
- AC-17 [v1.0]: Choosing any folder or file in the settings window opens the operating system's native dialog; no custom file browser exists.
- AC-18 [v1.0]: A file the daemon downloaded is not re-uploaded (no echo loop), and a file still being written is uploaded only after it has been unchanged for 2 s.
- AC-26 [v1.0]: The daemon syncs fully with no tray icon present. On Windows it starts through a logon task.
- AC-31 [v1.0]: Connecting a folder with 1,000 files of which 200 collide on path with the server (100 identical, 100 different) loses 0 files, uploads 0 identical bytes, and yields exactly 100 conflict copies.
- AC-32 [v1.0]: On a tree with 100 evicted iCloud or OneDrive files the daemon causes 0 bytes of hydration, reports 100 skipped files, and propagates 0 deletions.
- AC-33 [v1.0]: An exclusively locked file is retried without CPU spin (at most 1% CPU) and uploaded within 10 s after the lock is released; it is never reported as deleted.
- AC-34 [v1.0]: For save-in-place from vim, JetBrains, Word, LibreOffice and KeePassXC, the server shows exactly 1 new revision on the same file ID, 0 conflict copies, 0 deletes, and an existing share link still opens.
- AC-35 [v1.0]: After a 30-minute suspend with 500 files changed on a second device and locally, all devices converge within 60 s after wake on each supported OS.
- AC-37 [v1.0]: A 40-case hostile-name corpus per OS (traversal, alternate data streams, reserved names, bidi characters, symlink in path, junction, case collision) causes 0 writes outside the sync root and 0 overwritten files. The server rejects names containing `/`, NUL, `.` or `..` as a whole name, control characters and bidi-control characters, and accepts everything else; a client that cannot represent a name on its OS saves it under an escaped name and reports it.
- AC-48 [v1.0]: A different OS user, and on Windows a remote client, cannot open the daemon's IPC endpoint, and the IPC schema has no method that returns a token.
- AC-49 [v1.0]: The updater rejects an unsigned build, a build signed with another key and a lower version; the update source is fixed at build time and is never the sync server.

## Web client
- AC-50 [v1.0]: A 50 GiB upload from desktop Chrome, Firefox and Safari keeps worker plus page heap at most 150 MB; after closing the tab and re-selecting the file, at most 16 MiB is re-sent (the AC-3 bound, changed from 10 MiB on 2026-11-11 by the owner, AZ-3731); a file edited mid-upload is detected and the upload restarts.
- AC-51 [v1.0]: A 50 GiB download completes in Chrome, Firefox and Safari with browser heap at most 200 MB.

## Mobile
- AC-52 [v1.0 Android, v1.1 iOS]: One upload-protocol implementation serves every mobile entry point (reviewed in code). A background run starts uploading within 10 s and keeps memory at most 150 MB. A first sync of 50,000 assets survives 10 forced process kills with at most 1% of assets re-hashed and no duplicate server files.
- AC-53 [v1.0 Android, v1.1 iOS]: With limited or partial photo access the app shows "N photos visible, tap to allow more" and backs up newly picked photos on the next run.
- AC-54 [v1.0]: Apps refuse plain-HTTP sign-in by default.
- AC-66 [v1.0]: Include and exclude rules are gitignore-style patterns relative to the pair root (`*`, `**`, `?`, leading `/`, trailing `/` for folders, `!` negation). An excluded path is never uploaded, downloaded or deleted.
- AC-67 [v1.1]: When macOS denies access to a folder, the tray and settings window show "access denied: allow access in System Settings" within 10 s, other folders keep syncing, and that folder resumes within 10 s after permission is granted.

## Authentication and security
- AC-19 [v1.0 GitHub and generic OIDC, v1.1 Apple]: Users can sign in with each configured provider. Google works through generic OIDC. The operator can enable any subset, with at least one. The iOS app shows Google or GitHub sign-in only when Sign in with Apple is also enabled on that server.
- AC-20 [v1.0]: Abuse limits apply to defined failures: bad OAuth state or code, invalid tokens, wrong device-approval attempts (counted per approving session, per account and per IP), and wrong share passwords (per link). Excess attempts get HTTP 429 with Retry-After. Defaults: 5 wrong device-approval attempts per approving session, 20 per account per hour and 50 per IP per hour; 10 wrong share passwords per link per 10 min; 100 invalid share tokens per minute per IP; 10 failed sign-in steps per minute per IP.
- AC-21 [v1.0]: A revoked session or device stops syncing within 5 s, including live sync streams. With no internet access, existing sessions and device tokens keep working; only new sign-ins need the provider.
- AC-23 [v1.0]: On an empty database, a social login without the one-time setup token creates no account; the token works once and the `sunnycloud admin` command can create or reset admins. Later registration is by single-use invite token (never by email match). The operator may also list verified email addresses in configuration; an email is matched only at invite redemption.
- AC-55 [v1.0]: Account creation and provider linking never use an email address from a provider that does not verify it (such as Facebook or Apple); tested with a mock returning a victim's email unverified: 0 accounts created or linked.
- AC-56 [v1.0]: 12 negative OAuth callback tests pass (missing, replayed or foreign state; wrong provider, nonce, audience or issuer; absolute return URL; foreign Host header; reused one-time code; wrong code verifier; expired state).
- AC-57 [v1.0]: The device approval page shows the requester's IP and location against the approver's, warns on mismatch, expires the user code in at most 10 min, and device-grant tokens cannot call approve-device, create-share, delete-account or link-provider endpoints (403 in each test).
- AC-58 [v1.0]: A cross-origin POST with a valid cookie to every state-changing operation returns 403 (generated from OpenAPI), as does a state-changing request with a cookie but neither `Origin` nor `Sec-Fetch-Site`. Four concurrent refreshes with the same token yield one successor and no revocation; a replay after 30 s revokes the token family within 5 s.
- AC-59 [v1.0]: A generated two-account matrix over every API operation and every ID parameter shows 0 successful cross-account calls (all 404); CI fails when a new operation is not covered. Opening 1,000 stalled unauthenticated HTTP/2 connections raises server memory by at most 200 MB and delays an authenticated 1 GiB upload by at most 10%.
- AC-24 [v1.0]: A scheduled scrub re-verifies stored blobs, reports any mismatch, and is throttled so it does not degrade user transfers. Transfers keep at least 90% of their baseline throughput while the scrub runs.
- AC-25 [v1.0]: A decoder process has no database credentials, no blob-store mount and no network, and runs under CPU, memory and time limits; a red-team corpus (SVG or PDF renamed .png, an HLS playlist referencing `/etc/passwd`, a 30000x30000 PNG, a 20 GB sparse video, a truncated HEIC) ends in job failure without data exposure. Limits: photos 2 CPU, 2 GiB, 120 s; video 4 CPU, 4 GiB, 40 min.
- AC-27 [v1.0]: Deleting an account removes its files, thumbnails, transcodes, shares and sessions from the server. The deletion finishes within 15 min of the request.
- AC-63 [v1.0]: CI fails on any RustSec advisory without a dated, reasoned ignore, and ignores older than 90 days fail the build. Release images are signed with an SBOM; the worker image has 0 known HIGH or CRITICAL CVEs in ffmpeg, libheif, libvips and libwebp at release and is rebuilt at least every 7 days.

## Installation and operation
- AC-22 [v1.0]: An operator following the README, with one sign-in provider ready and the default Caddy reverse proxy in the Compose file, reaches a working server in at most 30 minutes.
- AC-64 [v1.0]: The API is versioned under `/api/v1`, and a client older than the minimum supported version gets a clear upgrade message instead of corrupt behavior.
- AC-65 [v1.0]: A documented backup and restore procedure (database, blobs and master key) is tested end to end in CI, and an upgrade between releases keeps all data.

## Interaction outcomes
- AC-68 [v1.0]: After the server is killed between writing a blob and committing it, no committed revision references a missing blob; the orphan blob is swept after 48 h and its reserved quota is released at session expiry.
- AC-69 [v1.0]: With 50 concurrent commits on one account, a device reading the change feed from cursor 0 sees every change exactly once, in order, with no skipped or repeated sequence number.
- AC-70 [v1.0]: When device A moves a folder while device B edits a file inside it, the edit ends up inside the moved folder with 0 conflict copies and an unchanged file ID.
- AC-71 [v1.0]: Killing the media supervisor mid-job leaves exactly one thumbnail or transcode per (account, content hash, parameters) after the retry; no duplicate derived files and no orphaned scratch data.
- AC-72 [v1.0]: A worker fetch of a 1 GiB blob through the internal endpoint equals the plaintext hash, and the server refuses to start when the master key does not match the stored key check.
- AC-73 [v1.0]: After a restore, `sunnycloud doctor` lists every revision whose blob is missing; those files show as unavailable to users and never produce server errors on listing.
- AC-74 [v1.0]: After 1,000 concurrent commits and deletes on one account, the stored usage equals the sum of its current live revisions and never exceeds the quota.
- AC-75 [v1.0]: Two concurrent redemptions of one invite token create exactly one account.
- AC-76 [v1.0]: A device code can be exchanged for a token exactly once; the second exchange fails, and the token cannot be minted after the code has expired.
- AC-77 [v1.0]: A share link follows its file across rename and move, and deleting the file makes the link answer as missing within 5 s.
- AC-78 [v1.0 Android]: When the foreground app and the background worker upload the same asset at once, exactly one server file is created.
- AC-79 [v1.0]: The internal worker endpoints reject user sessions and device tokens (403) and are not reachable through the public proxy route (404).
- AC-80 [v1.0]: The scrub never reports a blob that is being uploaded or removed by garbage collection as corrupt.

## Out of scope
- Version history, user-facing trash and restore.
- Video streaming (only transcoding).
- Local passwords.
- Facebook sign-in.
- Real-time collaborative editing.
- Files-on-demand placeholders on desktop.
- Cross-user deduplication.
