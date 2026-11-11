# Traceability Matrix

Generated from the coverage indexes of the five scenario files; `UNCOVERED` rows are assigned in "Uncovered Items Analysis". A row is `BLOCKED — missing data` when every test that traces to it is blocked.

## Acceptance Criteria Coverage

| AC ID | Release | Acceptance Criterion | Test IDs | Coverage |
|-------|---------|---------------------|----------|----------|
| AC-1 | v1.0 | One installation supports at least 500 registered users and 100 concurrent syncing sessions, tested at an agg… | NFT-PERF-01, NFT-RES-20, NFT-RES-21 | Covered |
| AC-2 | v1.0 | A single large-file transfer reaches at least 90% of available link speed, up to 1 Gbps (SSD or NVMe on both… | NFT-PERF-02, NFT-PERF-03, NFT-PERF-04, NFT-PERF-11, NFT-RES-02, NFT-SEC-15, NFT-SEC-18, NFT-SEC-23, NFT-SEC-24 | Covered |
| AC-3 | v1.0 | An interrupted upload resumes and re-sends at most 16 MiB (4 lanes x 2 MiB in flight plus one 8 MiB checkpoint interval). | FT-P-01, NFT-PERF-05, NFT-PERF-27, NFT-RES-01, NFT-RES-03, NFT-RES-04, NFT-RES-06, NFT-RES-26, NFT-RES-29 | Covered |
| AC-4 | v1.0 | Files up to 50 GiB can be uploaded and downloaded; finishing an upload takes constant time regardless of file… | FT-P-01, FT-P-03, FT-P-67, NFT-PERF-06, NFT-RES-01, NFT-RES-04, NFT-RES-05, NFT-SEC-20 | Covered |
| AC-5 | v1.0 | From the moment a file is closed on device A until device B holds the complete verified file, at most 5 s at… | FT-P-07, FT-P-08, NFT-PERF-08, NFT-PERF-09, NFT-RES-20, NFT-RES-21, NFT-SEC-26 | Covered |
| AC-6 | v1.0 | Server time to answer a listing of a folder with 10,000 entries is at most 300 ms at p95. The API pages large… | FT-P-09, NFT-PERF-10, NFT-RES-21 | Covered |
| AC-7 | v1.0 | Every transferred file is hash-verified end to end, with 0 undetected corruptions in transit. Corruption at r… | FT-N-01, FT-N-02, FT-P-01, FT-P-02, FT-P-03, FT-P-68, NFT-PERF-03, NFT-PERF-05, NFT-PERF-28, NFT-RES-01, NFT-RES-02, NFT-RES-03, NFT-SEC-14, NFT-SEC-20, NFT-SEC-21, NFT-SEC-22, NFT-SEC-25 | Covered |
| AC-8 | v1.0 | The later commit by server order (not client clock) becomes the current version, and the stored file is never… | FT-P-08, FT-P-10, FT-P-11, FT-P-12, NFT-RES-07, NFT-RES-08, NFT-RES-18, NFT-RES-20, NFT-RES-25, FT-P-77, FT-N-47 | Covered |
| AC-9 | v1.0 Android, v1.1 iOS | On Android, with battery optimization exempted and on Wi-Fi, a new photo uploads within 60 s of capture on a… | FT-P-29, FT-P-30, FT-P-31, NFT-PERF-12, NFT-RES-27, NFT-RES-33, FT-N-50 | Covered |
| AC-10 | v1.0 | For non-bulk uploads, a thumbnail for a 12 MP photo is ready within 5 s of upload completing, at p95, using a… | FT-N-07, FT-P-20, FT-P-21, FT-P-22, NFT-PERF-13, NFT-PERF-14, NFT-RES-19, NFT-RES-23, NFT-SEC-28, FT-N-49 | Covered |
| AC-11 | v1.0 | Transcode workers run on the operator's own machines. On a CPU-only reference worker with 4 physical cores: a… | FT-P-23, FT-P-24, FT-P-25, NFT-PERF-15, NFT-PERF-16, NFT-PERF-17, NFT-PERF-19, NFT-RES-19, NFT-RES-23 | Covered |
| AC-12 | v1.0 | A share link opens in at most 2 s, expires after 7 days by default (configurable), and revocation takes effec… | FT-N-09, FT-N-10, FT-N-44, FT-P-36, FT-P-37, FT-P-38, NFT-RES-25, NFT-SEC-06, NFT-SEC-11, NFT-SEC-16, NFT-SEC-17, NFT-SEC-18, NFT-SEC-19, FT-P-80, NFT-SEC-51 | Covered |
| AC-13 | v1.2 | The server stores only ciphertext for vault files. A vault file downloaded from the server differs byte for b… | FT-N-13, FT-P-44, NFT-SEC-39, NFT-SEC-41, NFT-SEC-42, NFT-SEC-43 | Covered |
| AC-14 | v1.2 | Vault files up to 100 MiB (configurable) are supported; larger ones are rejected with a clear message. Camera… | FT-N-14, FT-N-37, FT-P-44, FT-P-45, NFT-PERF-29, NFT-RES-LIM-08, NFT-SEC-43 | Covered |
| AC-15 | v1.0 | With 100,000 watched files, the daemon uses at most 1% CPU when idle and at most 100 MB memory; the first sca… | NFT-PERF-24, NFT-RES-LIM-06, NFT-RES-40 | Covered |
| AC-16 | v1.0 | The settings window opens within 1 s (cold start at p95 on a 2019 Windows laptop with an HDD-class disk and o… | FT-P-48, FT-P-49, NFT-PERF-25, NFT-PERF-26, FT-P-82 | Covered |
| AC-17 | v1.0 | Choosing any folder or file in the settings window opens the operating system's native dialog; no custom file… | FT-P-50 | Covered |
| AC-18 | v1.0 | A file the daemon downloaded is not re-uploaded (no echo loop), and a file still being written is uploaded on… | FT-P-51, FT-P-52 | Covered |
| AC-19 | v1.0 GitHub and generic OIDC, v1.1 Apple | Users can sign in with each configured provider. Google works through generic OIDC. The operator can enable a… | FT-N-25, FT-N-34, FT-P-62, FT-P-63, NFT-RES-24, NFT-RES-32, NFT-SEC-02, NFT-SEC-30 | Covered |
| AC-20 | v1.0 | Abuse limits apply to defined failures: bad OAuth state or code, invalid tokens, wrong device-approval attemp… | NFT-SEC-01, NFT-SEC-04, NFT-SEC-05, NFT-SEC-06, NFT-SEC-17, FT-P-79 | Covered |
| AC-21 | v1.0 | A revoked session or device stops syncing within 5 s, including live sync streams. With no internet access, e… | FT-N-30, NFT-RES-16, NFT-RES-20, NFT-RES-24, NFT-RES-26, NFT-RES-32, NFT-SEC-07, NFT-SEC-08, NFT-SEC-10, NFT-SEC-27, NFT-SEC-34, NFT-SEC-44, NFT-SEC-47, NFT-RES-41 | Covered |
| AC-22 | v1.0 | An operator following the README, with one sign-in provider ready and the default Caddy reverse proxy in the… | FT-P-69, NFT-PERF-30 | Covered |
| AC-23 | v1.0 | On an empty database, a social login without the one-time setup token creates no account; the token works onc… | FT-N-20, FT-N-21, FT-P-64, FT-P-65, NFT-SEC-01, NFT-SEC-02, NFT-SEC-38, NFT-SEC-48, FT-P-78, FT-N-46 | Covered |
| AC-24 | v1.0 | A scheduled scrub re-verifies stored blobs, reports any mismatch, and is throttled so it does not degrade use… | FT-P-43, NFT-RES-04, NFT-RES-10, NFT-RES-39, NFT-RES-LIM-05, NFT-SEC-25, NFT-SEC-26 | Covered |
| AC-25 | v1.0 | A decoder process has no database credentials, no blob-store mount and no network, and runs under CPU, memory… | FT-N-07, NFT-RES-23, NFT-RES-LIM-04, NFT-RES-LIM-09, NFT-SEC-28, NFT-SEC-29, NFT-SEC-45, NFT-SEC-46 | Covered |
| AC-26 | v1.0 | The daemon syncs fully with no tray icon present. On Windows it starts through a logon task. | FT-P-53 | Covered |
| AC-27 | v1.0 | Deleting an account removes its files, thumbnails, transcodes, shares and sessions from the server. The delet… | FT-P-19, NFT-RES-30, NFT-SEC-27, NFT-RES-41 | Covered |
| AC-28 | v1.1 | A worker detects an NVIDIA NVENC encoder by a test encode at start and uses it only when the operator enables… | FT-N-08, FT-P-28, NFT-PERF-18, NFT-PERF-19, NFT-RES-31, NFT-RES-LIM-09 | Covered |
| AC-29 | v1.0 | If the sync root is missing, unmounted or empty, or 200 or more deletions arrive in one batch, the desktop cl… | FT-N-17, FT-N-18, FT-P-59, NFT-RES-11, NFT-RES-12 | Covered |
| AC-30 | v1.0 | A blob unreferenced by a delete or overwrite is not removed before the retention period (default 7 days, conf… | FT-N-35, FT-P-16, FT-P-17, FT-P-18, NFT-RES-08, NFT-RES-09, NFT-RES-10, NFT-RES-30, NFT-SEC-27, NFT-SEC-38, NFT-SEC-47, NFT-RES-42 | Covered |
| AC-31 | v1.0 | Connecting a folder with 1,000 files of which 200 collide on path with the server (100 identical, 100 differe… | FT-P-54, NFT-RES-18 | Covered |
| AC-32 | v1.0 | On a tree with 100 evicted iCloud or OneDrive files the daemon causes 0 bytes of hydration, reports 100 skipp… | FT-P-55, NFT-RES-17 | Covered |
| AC-33 | v1.0 | An exclusively locked file is retried without CPU spin (at most 1% CPU) and uploaded within 10 s after the lo… | FT-P-56, NFT-RES-15, NFT-RES-LIM-07 | Covered |
| AC-34 | v1.0 | For save-in-place from vim, JetBrains, Word, LibreOffice and KeePassXC, the server shows exactly 1 new revisi… | FT-P-57 | Covered |
| AC-35 | v1.0 | After a 30-minute suspend with 500 files changed on a second device and locally, all devices converge within… | FT-P-58, NFT-RES-16, NFT-RES-25, FT-P-77 | Covered |
| AC-37 | v1.0 | A 40-case hostile-name corpus per OS (traversal, alternate data streams, reserved names, bidi characters, sym… | FT-N-19, FT-N-41, NFT-SEC-22, NFT-SEC-31, FT-N-51 | Covered |
| AC-38 | v1.0 | A block is acknowledged only after it is flushed to disk. After `kill -9` of the server during 100 concurrent… | FT-P-06, NFT-RES-03, NFT-RES-04, NFT-RES-06, NFT-RES-22, NFT-SEC-20 | Covered |
| AC-39 | v1.0 | 100 upload sessions that each declare 50 GiB are refused beyond the per-account session cap and the quota, an… | NFT-RES-05, NFT-RES-06, NFT-RES-LIM-01, NFT-SEC-20, FT-P-79 | Covered |
| AC-40 | v1.0 | 10,000 files of up to 1 MiB upload in at most 60 s at 20 ms round-trip time, using single-request upload for… | FT-N-38, FT-P-04, FT-P-05, NFT-PERF-07, NFT-RES-22, FT-N-48 | Covered |
| AC-41 | v1.0 | After the database is restored from an older backup, every device receives `resync_required` and converges wi… | FT-P-13, FT-P-14, NFT-RES-07, NFT-RES-08, NFT-RES-13 | Covered |
| AC-42 | v1.0 | Names are unique per folder case-insensitively and are stored case-preserving; two names differing only by ca… | FT-N-04, FT-N-05, FT-N-41, FT-P-15, NFT-RES-18, NFT-SEC-22, NFT-SEC-31 | Covered |
| AC-43 | v1.0 | Blobs are encrypted on disk by the server with per-blob keys wrapped by an operator-supplied master key. A st… | FT-N-12, FT-N-33, FT-N-43, FT-P-40, FT-P-41, FT-P-42, FT-P-71, NFT-PERF-02, NFT-PERF-03, NFT-PERF-11, NFT-RES-13, NFT-SEC-23, NFT-SEC-24, NFT-SEC-25 | Covered |
| AC-44 | v1.0 | On a corpus of 100 mixed phone clips, derived storage (thumbnails plus transcodes) is at most 20% of original… | FT-P-25, FT-P-26, NFT-PERF-17, NFT-PERF-20, NFT-RES-19 | Covered |
| AC-45 | v1.0 | A stalled ffmpeg is killed within 90 s of its last progress line and retried once on CPU. Rotated and variabl… | FT-N-06, FT-P-27, NFT-PERF-21, NFT-RES-19 | Covered |
| AC-46 | v1.0 | Re-installing the Android app on a phone with 5,000 already-backed-up assets uploads 0 bytes of media; an edi… | FT-P-32, FT-P-33, NFT-PERF-22, NFT-RES-28, NFT-SEC-43 | Covered |
| AC-47 | v1.0 | Inline display is allowed only for image/jpeg, image/png, image/gif, image/webp, image/avif, video/mp4 and vi… | FT-N-11, FT-P-39, NFT-SEC-19 | Covered |
| AC-48 | v1.0 | A different OS user, and on Windows a remote client, cannot open the daemon's IPC endpoint, and the IPC schem… | NFT-SEC-32, NFT-SEC-34 | Covered |
| AC-49 | v1.0 | The updater rejects an unsigned build, a build signed with another key and a lower version; the update source… | NFT-SEC-33 | Covered |
| AC-50 | v1.0 | A 50 GiB upload from desktop Chrome, Firefox and Safari keeps worker plus page heap at most 150 MB; after clo… | FT-N-03, FT-P-67, NFT-PERF-27, NFT-RES-29 | Covered |
| AC-51 | v1.0 | A 50 GiB download completes in Chrome, Firefox and Safari with browser heap at most 200 MB. | FT-P-68, NFT-PERF-28 | Covered |
| AC-52 | v1.0 Android, v1.1 iOS | One upload-protocol implementation serves every mobile entry point (reviewed in code). A background run start… | FT-P-34, NFT-PERF-23, NFT-RES-27, NFT-RES-28, NFT-RES-LIM-03, NFT-RES-33, FT-N-50 | Covered |
| AC-53 | v1.0 Android, v1.1 iOS | With limited or partial photo access the app shows "N photos visible, tap to allow more" and backs up newly picked photos on the n… | FT-P-35 | Covered |
| AC-54 | v1.0 | Apps refuse plain-HTTP sign-in by default. | FT-N-26, NFT-SEC-11, NFT-SEC-12 | Covered |
| AC-55 | v1.0 | Account creation and provider linking never use an email address from a provider that does not verify it (suc… | FT-N-22, NFT-SEC-02 | Covered |
| AC-56 | v1.0 | 12 negative OAuth callback tests pass (missing, replayed or foreign state; wrong provider, nonce, audience or… | FT-N-23, NFT-SEC-03, NFT-SEC-30 | Covered |
| AC-57 | v1.0 | The device approval page shows the requester's IP and location against the approver's, warns on mismatch, exp… | FT-N-24, FT-P-66, NFT-SEC-05, NFT-SEC-07, NFT-SEC-49, FT-P-81, FT-N-45 | Covered |
| AC-58 | v1.0 | A cross-origin POST with a valid cookie to every state-changing operation returns 403 (generated from OpenAPI… | NFT-SEC-09, NFT-SEC-10, NFT-RES-33 | Covered |
| AC-59 | v1.0 | A generated two-account matrix over every API operation and every ID parameter shows 0 successful cross-accou… | NFT-RES-LIM-02, NFT-SEC-13, NFT-SEC-14, NFT-SEC-15 | Covered |
| AC-60 | v1.2 | Moving a wrapped key and its ciphertext to another file, vault or parent folder is rejected in 1,000 of 1,000… | FT-N-15, FT-P-46, NFT-SEC-40 | Covered |
| AC-61 | v1.2 | The client rejects vault headers with memory below 64 MiB or above 512 MiB, passes below 3, parallelism other… | FT-N-16, NFT-SEC-41 | Covered |
| AC-62 | v1.2 | The web vault page serves a CSP without `unsafe-inline` or `unsafe-eval`, loads 0 third-party hosts, and afte… | FT-P-47, NFT-PERF-29, NFT-RES-LIM-08, NFT-SEC-42 | Covered |
| AC-63 | v1.0 | CI fails on any RustSec advisory without a dated, reasoned ignore, and ignores older than 90 days fail the bu… | NFT-SEC-35, NFT-SEC-36, NFT-SEC-37 | Covered |
| AC-64 | v1.0 | The API is versioned under `/api/v1`, and a client older than the minimum supported version gets a clear upgr… | FT-N-27, FT-P-70, NFT-RES-13, FT-P-79 | Covered |
| AC-65 | v1.0 | A documented backup and restore procedure (database, blobs and master key) is tested end to end in CI, and an… | FT-N-33, FT-N-39, FT-P-71, FT-P-72, NFT-RES-10, NFT-RES-13, NFT-RES-14, NFT-RES-37, NFT-SEC-46 | Covered |
| AC-66 | v1.0 | Include and exclude rules are gitignore-style patterns relative to the pair root (`*`, `**`, `?`, leading `/`… | FT-P-60 | Covered |
| AC-67 | v1.1 | When macOS denies access to a folder, the tray and settings window show "access denied: allow access in Syste… | FT-P-75 | Covered |
| AC-68 | v1.0 | After the server is killed between writing a blob and committing it, no committed revision references a… | NFT-RES-34 | Covered |
| AC-69 | v1.0 | With 50 concurrent commits on one account, a device reading the change feed from cursor 0 sees every cha… | NFT-RES-35 | Covered |
| AC-70 | v1.0 | When device A moves a folder while device B edits a file inside it, the edit ends up inside the moved fo… | FT-N-42 | Covered |
| AC-71 | v1.0 | Killing the media supervisor mid-job leaves exactly one thumbnail or transcode per (account, content has… | NFT-RES-36, FT-N-49 | Covered |
| AC-72 | v1.0 | A worker fetch of a 1 GiB blob through the internal endpoint equals the plaintext hash, and the server r… | FT-N-43 | Covered |
| AC-73 | v1.0 | After a restore, `sunnycloud doctor` lists every revision whose blob is missing; those files show as una… | NFT-RES-37 | Covered |
| AC-74 | v1.0 | After 1,000 concurrent commits and deletes on one account, the stored usage equals the sum of its curren… | NFT-RES-38 | Covered |
| AC-75 | v1.0 | Two concurrent redemptions of one invite token create exactly one account. | NFT-SEC-48 | Covered |
| AC-76 | v1.0 | A device code can be exchanged for a token exactly once; the second exchange fails, and the token cannot… | NFT-SEC-49, FT-N-45 | Covered |
| AC-77 | v1.0 | A share link follows its file across rename and move, and deleting the file makes the link answer as mis… | FT-N-44 | Covered |
| AC-78 | v1.0 Android | When the foreground app and the background worker upload the same asset at once, exactly one server file is created. | FT-P-76 | Covered |
| AC-79 | v1.0 | The internal worker endpoints reject user sessions and device tokens (403) and are not reachable through… | NFT-SEC-50 | Covered |
| AC-80 | v1.0 | The scrub never reports a blob that is being uploaded or removed by garbage collection as corrupt. | NFT-RES-39 | Covered |

## Restrictions Coverage

| Restriction ID | Restriction | Test IDs | Coverage |
|---------------|-------------|----------|----------|
| RESTRICT-01 | v1.0: server, web (Svelte 5), Android, Linux and Windows desktop, GitHub and generic-OIDC sign-in (Google thr… | FT-P-73, not in this file's area, not in this file's area (see functional, performance, security tests) | Covered |
| RESTRICT-02 | v1.1: iOS, macOS desktop, Sign in with Apple, optional NVIDIA GPU encoding. | FT-P-28, FT-P-63, FT-P-73, FT-P-75, NFT-PERF-18, NFT-PERF-19 | Covered |
| RESTRICT-03 | v1.2: end-to-end encrypted vault, released only after an external review. | FT-P-73 | Covered |
| RESTRICT-04 | Out of scope: Facebook sign-in, Intel/AMD/Apple GPU encoding (documented as experimental only). | FT-N-08, FT-N-28, FT-P-73 | Covered |
| RESTRICT-05 | Rust for the API, background workers and desktop daemons. Svelte 5 for the web client. Flutter for Android an… | FT-P-70 | Covered |
| RESTRICT-06 | Desktop (Linux, Windows, later macOS): native Rust daemon with a tray icon and a small settings window built… | FT-P-48, FT-P-50, FT-P-60, NFT-PERF-24, NFT-PERF-25, NFT-PERF-26, NFT-RES-LIM-06, NFT-SEC-31, NFT-SEC-32 | Covered |
| RESTRICT-07 | Server storage: immutable blobs named by ID on XFS (documented default) or ext4 (`blobs/<2hex>/<2hex>/<id>`),… | FT-N-04, FT-P-15, FT-P-40, NFT-PERF-01, NFT-PERF-09, NFT-PERF-10, NFT-RES-03, NFT-RES-04, NFT-RES-06, NFT-RES-34, NFT-RES-39, NFT-SEC-13, NFT-SEC-14, NFT-SEC-22, NFT-SEC-26 | Covered |
| RESTRICT-08 | Blobs are encrypted at rest by the server with per-blob keys wrapped by an operator-supplied master key (enve… | FT-N-12, FT-N-33, FT-N-43, FT-P-40, NFT-PERF-02, NFT-PERF-03, NFT-PERF-11, NFT-RES-13, NFT-RES-LIM-05, NFT-SEC-23, NFT-SEC-24, NFT-SEC-25 | Covered |
| RESTRICT-09 | A block counts as received only after a batched fdatasync. Upload sessions reserve quota at start, are capped… | FT-N-40, FT-P-06, FT-P-13, FT-P-14, NFT-PERF-01, NFT-PERF-05, NFT-PERF-06, NFT-PERF-09, NFT-RES-01, NFT-RES-03, NFT-RES-04, NFT-RES-05, NFT-RES-06, NFT-RES-07, NFT-RES-08, NFT-RES-20, NFT-RES-34, NFT-RES-35, NFT-RES-38, NFT-RES-LIM-01, NFT-SEC-20 | Covered |
| RESTRICT-10 | Replaced and deleted blobs are kept for a retention period (default 7 days, 0 to 90) through a garbage-collec… | FT-N-31, FT-P-16, FT-P-17, FT-P-19, NFT-RES-09, NFT-RES-10, NFT-RES-30, NFT-RES-37, NFT-RES-38, NFT-RES-39, NFT-SEC-27, NFT-RES-42 | Covered |
| RESTRICT-11 | Sync protocol: plain HTTPS over TCP (TLS 1.3, HTTP/2) with fixed 2 MiB blocks, at most 4 parallel lanes, BLAK… | FT-N-32, FT-P-01, FT-P-04, FT-P-05, FT-P-70, NFT-PERF-01, NFT-PERF-04, NFT-PERF-05, NFT-PERF-06, NFT-PERF-07, NFT-PERF-08, NFT-PERF-09, NFT-PERF-10, NFT-PERF-27, NFT-PERF-28, NFT-RES-01, NFT-RES-02, NFT-RES-20, NFT-RES-21, NFT-RES-22, NFT-RES-25, NFT-RES-35, NFT-RES-LIM-02, NFT-SEC-09, NFT-SEC-11, NFT-SEC-13, NFT-SEC-14, NFT-SEC-15, NFT-SEC-16, NFT-SEC-19, NFT-SEC-20, NFT-SEC-21 | Covered |
| RESTRICT-12 | Dependency floors: tokio 1.54.1 or later (1.54.0 was yanked), h2 0.4.16 or later, bytes 1.11.1 or later, rust… | NFT-RES-LIM-02, NFT-SEC-15, NFT-SEC-35, NFT-SEC-36, NFT-SEC-37 | Covered |
| RESTRICT-13 | Media: libvips thumbnails (`vipsthumbnail`) and `ffmpeg` as subprocesses. ffmpeg is built without `libfdk_aac… | 20, 21, 30, FT-P-23, FT-P-25, FT-P-69, FT-P-74, NFT-PERF-13 to 17, NFT-RES-19, NFT-RES-23, NFT-RES-LIM-04, NFT-RES-LIM-09, NFT-SEC-28, NFT-SEC-29, NFT-SEC-36, NFT-SEC-45, NFT-SEC-46, FT-N-43, NFT-RES-36, NFT-SEC-50 | Covered |
| RESTRICT-14 | NVIDIA NVENC is opt-in and off by default [v1.1]; CPU is the acceptance baseline. | FT-N-08, FT-P-28, FT-P-73, NFT-PERF-15, NFT-PERF-18, NFT-PERF-19, NFT-RES-31, NFT-RES-LIM-09 | Covered |
| RESTRICT-15 | Mobile backup: Android uses native WorkManager with a MediaStore trigger and user-initiated data-transfer job… | FT-P-29, FT-P-34, FT-P-76, NFT-PERF-12, NFT-PERF-22, NFT-PERF-23, NFT-RES-27, NFT-RES-LIM-03 | Covered |
| RESTRICT-16 | Docker Compose is the install; it ships Caddy as the default reverse proxy with request buffering off and rai… | FT-P-69, NFT-PERF-01, NFT-PERF-02, NFT-PERF-03, NFT-PERF-30, NFT-RES-01, NFT-SEC-11, NFT-SEC-50 | Covered |
| RESTRICT-17 | Built-in OAuth/OIDC client in the Rust API; identity is (provider, provider user id), never email. Providers:… | FT-N-29, FT-P-62, NFT-RES-24, NFT-SEC-02, NFT-SEC-03, NFT-SEC-04, NFT-SEC-30 | Covered |
| RESTRICT-18 | The first admin is created with a one-time setup token and the `sunnycloud admin` command. Later registration… | FT-N-20, FT-N-21, FT-N-22, FT-P-64, FT-P-65, NFT-SEC-01, NFT-SEC-02, NFT-SEC-38, NFT-SEC-48 | Covered |
| RESTRICT-19 | The server issues its own opaque sessions with refresh rotation and a reuse grace window, plus its own RFC 86… | FT-N-24, FT-P-61, FT-P-66, NFT-RES-24, NFT-RES-26, NFT-RES-32, NFT-SEC-05, NFT-SEC-07, NFT-SEC-08, NFT-SEC-09, NFT-SEC-10, NFT-SEC-34, NFT-SEC-38, NFT-SEC-44, NFT-RES-33, NFT-SEC-49 | Covered |
| RESTRICT-20 | A server with no internet access cannot authenticate users. Operators register their own provider apps. | FT-N-30, NFT-RES-24, NFT-RES-32, NFT-SEC-03, NFT-SEC-30 | Covered |
| RESTRICT-21 | The iOS app shows Google or GitHub sign-in only when the server also has Sign in with Apple enabled (App Stor… | FT-N-34, FT-P-63 | Covered |
| RESTRICT-22 | TLS for all traffic. Vault folders are end-to-end encrypted with a passphrase separate from the login and a o… | FT-N-26, FT-N-32, FT-N-37, FT-P-44, NFT-SEC-11, NFT-SEC-12, NFT-SEC-39, NFT-SEC-42 | Covered |
| RESTRICT-23 | Vault crypto is one shared Rust core compiled native, to WASM and through UniFFI: chunked XChaCh… | FT-P-44, NFT-PERF-29, NFT-RES-LIM-08, NFT-SEC-39, NFT-SEC-40, NFT-SEC-41, NFT-SEC-42, NFT-SEC-43 | Covered |
| RESTRICT-24 | Last commit wins; a stale edit is kept as a conflict copy; an edit beats a delete; no version history or user… | FT-N-17, FT-N-31, FT-N-42, FT-P-10, FT-P-12, FT-P-55, FT-P-59, NFT-RES-11, NFT-RES-12, NFT-RES-15, NFT-RES-17, NFT-RES-18, NFT-RES-25, NFT-SEC-31 | Covered |
| RESTRICT-25 | Desktop daemons run at login without UI: systemd user service (Linux), Windows logon task (a service cannot o… | FT-P-53, NFT-PERF-24, NFT-RES-LIM-06 | Covered |
| RESTRICT-26 | macOS: the user grants file access once in system settings when syncing protected locations. | FT-P-75 | Covered |
| RESTRICT-27 | iOS limits background work, so camera backup there is best effort. | FT-P-31 | Covered |
| RESTRICT-28 | The operator installs on their own infrastructure; hundreds of users per install; transcode workers on operat… | FT-N-31, NFT-PERF-17, NFT-RES-23, NFT-SEC-29, NFT-SEC-45 | Covered |
| RESTRICT-29 | Licence: AGPL-3.0 for server, workers, desktop and apps, with an App Store exception file. The worker image s… | FT-P-74, NFT-SEC-36 | Covered |
| RESTRICT-30 | Builds on GitHub Actions with actions pinned by commit SHA and image digests pinned; fallback is git.azaion.c… | NFT-SEC-33, NFT-SEC-37 | Covered |
| RESTRICT-31 | Backup and restore (database, blobs, master key) and release upgrades are documented and tested. | FT-N-33, FT-N-39, FT-P-71, FT-P-72, NFT-RES-07, NFT-RES-10, NFT-RES-13, NFT-RES-14, NFT-RES-37 | Covered |
| RESTRICT-32 | Thumbnails and transcodes are encrypted at rest like every other blob. Job leases last 5 min with 3 attempts;… | FT-N-32, FT-N-38, FT-N-39, FT-N-40, FT-P-40, NFT-RES-34, NFT-RES-36 | Covered |
| RESTRICT-33 | The Android device token is excluded from backups. The vault passphrase must have at least 12 characters and… | FT-N-36, FT-N-37 | Covered |

## Coverage Summary

| Category | Total Items | Covered | BLOCKED | UNCOVERED | WAIVED |
|----------|-----------|---------|---------|-----------|--------|
| Acceptance Criteria (AC-1..80; AC-36 does not exist) | 79 | 79 | 0 | 0 | 0 |
| Restrictions | 33 | 33 | 0 | 0 | 0 |
| **Total** | 112 | 112 | 0 | 0 | 0 |

Scenarios: 265 total (blackbox 133, resilience 42, security 51, performance 30, resource-limit 9; 16 added by the architecture-sync pass of 2026-11-04); 0 marked BLOCKED. `acceptance_criteria.md` holds 79 ACs, all with at least one scenario. The 12 interaction ACs (AC-68 to AC-80 except AC-78) were covered in plan Step 5 by FT-N-42, FT-N-43, FT-N-44, NFT-RES-34 to NFT-RES-39 and NFT-SEC-48 to NFT-SEC-50; AC-78 was covered earlier by FT-P-76.

## Interaction ACs (AC-68 to AC-80): owners

Owner = owning component per `interaction-risks.md` (section "Component-owned interaction ACs"); contributors in brackets.

| AC ID | Release | Owning component (contributors) | Scenario |
|-------|---------|--------------------------------|----------|
| AC-68 | v1.0 | 02 tree (03 transfer, 04 blobstore file-before-row, 07a quota release) | NFT-RES-34 |
| AC-69 | v1.0 | 02 tree (03 transfer, 05 sync_feed; client side 13 sync_core) | NFT-RES-35 |
| AC-70 | v1.0 | 02 tree (03 transfer) | FT-N-42 |
| AC-71 | v1.0 | 08 media_jobs (09 worker_supervisor, 10 decoder) | NFT-RES-36 |
| AC-72 | v1.0 | 04 blobstore (12 operator_cli: `key-check`; 08 internal worker fetch) | FT-N-43 |
| AC-73 | v1.0 | 04 blobstore (12 operator_cli: `doctor`; 02 `TreeReader::describe_blobs`) | NFT-RES-37 |
| AC-74 | v1.0 | 07a quota (02 tree and 03 transfer: commit, delete, reserve and release) | NFT-RES-38 |
| AC-75 | v1.0 | 01 identity | NFT-SEC-48 |
| AC-76 | v1.0 | 01 identity | NFT-SEC-49 |
| AC-77 | v1.0 | 02 tree (06 sharing) | FT-N-44 |
| AC-78 | v1.0 Android | 17 android_app `mobile_backup` (13 sync_core) | FT-P-76 |
| AC-79 | v1.0 | 08 media_jobs, worker authentication (11 http_api, listener separation and absent public routes) | NFT-SEC-50 |
| AC-80 | v1.0 | 04 blobstore (12 operator_cli: `scrub run`) | NFT-RES-39 |

Corrections to the earlier pending table: AC-74 is owned by 07a (not 02), and AC-79 by 08 with 11 as contributor (not 01 identity).

## Uncovered Items Analysis

| Item | Status | Reason | Risk | Mitigation |
|------|--------|--------|------|-----------|

## Architecture-sync additions (greenfield Step 5, 2026-11-04)

Inputs: `architecture.md`, `system-flows.md` error rows, component interfaces and the owner-approved `ui_mockups/` (DESIGN.md §8). Each flow error row without a system-level scenario got one; covered rows were left alone. Coverage stays 112 of 112.

| Test ID | Source | Traces to |
|---------|--------|-----------|
| FT-P-78 | F1 "Token lost from log"; 12 `admin setup-token` | AC-23 |
| FT-P-79 | 16 §5 error handling; mockup `web-files.html` "Errors", `web-public-share.html` "Rate limited" | AC-64, AC-39, AC-20 |
| FT-P-80 | 06 `create` (`TargetNotShareable`); mockup `web-share-dialog.html` "Link created", "Folder" | AC-12 |
| FT-P-81 | mockup `web-device-approval.html` "Different location" (deny is primary, approve needs confirmation) | AC-57 |
| FT-P-82 | 15 §5 "No hidden state"; mockup `desktop-settings.html` Settings tab | AC-16 |
| FT-N-45 | F3 "User denies", "Polling too fast" | AC-57, AC-76 |
| FT-N-46 | 07 `LastAdmin` (409); mockup `web-admin.html` "Last admin" | AC-23 |
| FT-N-47 | F8 "Delete of an already deleted node" | AC-8 |
| FT-N-48 | F5 "One entry has an invalid name or stale base", "Batch too large" | AC-40 |
| FT-N-49 | F10 "Revision superseded or file deleted while queued" | AC-10, AC-71 |
| FT-N-50 | F16 "Asset deleted between listing and upload", "Quota exceeded"; mockup `android-backup.html` "Needs attention" | AC-9, AC-52 |
| FT-N-51 | F9 "Server rejects a name" | AC-37 |
| NFT-RES-40 | F9 "Watcher overflow" (Linux watch limit, rescan-only) | AC-15 |
| NFT-RES-41 | F13 "In-flight upload commits" | AC-27, AC-21 |
| NFT-RES-42 | F12 "Crash between unlink and row delete" | AC-30, RESTRICT-10 |
| NFT-SEC-51 | 06 `open` (`PasswordRequired` has no file fields); mockup `web-public-share.html` "Password" | AC-12 |

Not added: the F15 "Server stops mid-run" row (oldest-first resume needs a new read-only probe; scrub coverage stays with FT-P-43, NFT-SEC-26 and NFT-RES-39) and UI states that only restate covered behaviour (verified and unverified download labels: FT-P-68; limited photo access: FT-P-35). New owner decisions from the mockups: `spec-gaps.md` G-AS-1 and G-AS-2.

<!-- autodev-step5-post-architecture-pass: 2026-11-04 -->
