# Acceptance Criteria Assessment

Sources: `raw/storage_db.md`, `raw/sync_protocol.md`, `raw/media_pipeline.md`, `raw/security_clients.md` (each has its own numbered sources and fact cards). Status: proposed, waiting for the owner's confirmation. Numbers marked "estimate" are unmeasured.

## Acceptance Criteria

| Criterion | Our Values | Researched Values | Cost/Timeline Impact | Status |
|-----------|-----------|-------------------|---------------------|--------|
| AC-1 scale | 500 users, 100 concurrent | One Postgres node handles it; 500 users x 50k files = 25M rows, ~10-15 GB DB (analysis). Limit is network and disk, not the DB | Define load as "100 concurrent sessions at X MB/s aggregate" | Modified |
| AC-2 throughput | >= 90% of link, up to 1 Gbps | TCP goodput ceiling ~94% on 1500 MTU; TLS/HTTP/2 overhead <0.2%; HTTP/3 is slower for bulk (up to 45% less than HTTP/2, WWW'24); reverse-proxy request buffering would halve it | Test conditions: SSD/NVMe both sides, RTT <= 20 ms, file >= 1 GiB, proxy buffering off. HDD servers may cap below 112 MB/s | Modified |
| AC-3 resume | re-send <= 10 MiB | 2 MiB fixed blocks x <= 4 parallel lanes = 8 MiB worst case; 4 MiB blocks would break it | Needs session expiry (24 h-7 d) and server-side commit-before-ack | Kept |
| AC-4 50 GiB | 50 GiB | Single pre-sized staging file finished by rename avoids a copy; Nextcloud 43 GB upload failed at synchronous assembly behind a 100 s proxy timeout. Blocklist ~800 KiB per 50 GiB file | Proxy body-size limits raised; finalize is O(1) | Kept |
| AC-5 sync latency | <= 5 s p95, <= 10 MB | Budget 2.4-3.6 s, of which 2 s is the AC-18 settle time; holds with long-poll/SSE, fails with 30 s polling | No extra debounce on top of AC-18 | Kept |
| AC-6 list 10,000 entries | <= 300 ms p95 | Index scan is milliseconds; 10,000 entries is ~2.5 MB of JSON, so payload and network dominate | Measure server time only; API stays paged | Modified |
| AC-7 integrity | 0 silent corruption | In flight: end-to-end hash achieves it. At rest: ext4/XFS have no data checksums; a field study found >400k silent mismatches over 1.5M drives | Restate: any corruption is detected by a scheduled scrub and reported; repair from backup or a ZFS/btrfs mirror. ECC RAM recommended | Modified |
| AC-8 conflicts | later write wins, no mix | Server-assigned commit order per account; client clocks are not trusted | Atomic temp-file + rename | Kept |
| AC-9 camera backup | Android <= 60 s; iOS <= 15 min | Android: seconds to minutes when battery optimization is off; OEM killers push it to hours. iOS: background is OS-scheduled with no latency promise (iOS 26.1+ adds a system upload extension, still no SLA) | iOS stays best effort with no number. Android target needs the battery-exemption wording | Modified |
| AC-10 thumbnail | <= 5 s p95 | Resize is 6-16 ms; ~50-150 ms per 12 MP JPEG, HEIC ~0.3-0.6 s (estimates). Real risk is queue wait during a bulk backlog | Reserve a thumbnail lane (>= 1 vCPU); scope the AC to non-bulk uploads | Modified |
| AC-11 transcode (updated: operator hardware, owner note) | 10 min 1080p in <= 10 min on a 4-physical-core CPU worker; <= 3 min with a GPU | x264 1080p: 61 fps on a 4C/8T desktop, 26 fps on an Intel N100; 18,000 frames = 5-11.5 min. A cloud 4-vCPU is often 2 physical cores | Realistic only at preset veryfast/fast, CRF ~23, 1080p cap, 30 fps source. AV1 is too slow on CPU | Modified |
| AC-28 GPU encode | (new, owner request) | Workers run on operator hardware and should use existing GPUs: NVENC, VAAPI/QSV, VideoToolbox; GPU encode is typically several times faster than CPU x264 but less bitrate-efficient | Detect at start, fall back to CPU; container needs device passthrough | Added |
| AC-12 share links | open <= 2 s, revoke <= 5 s | Revocation is a DB row check; do not cache link state beyond a few seconds | | Kept |
| AC-13 vault ciphertext | server stores only ciphertext | Achievable (Cryptomator, Ente). Remaining risk is metadata leakage and a malicious server | Add tests: tampered, truncated, reordered or wrong-vault chunks are rejected; document what leaks (sizes, counts, tree shape, timing) | Modified |
| AC-14 vault file limit | 100 MiB | Chunked AEAD has no such limit; the cap is about browser memory. 100 MiB rejects any phone video over ~1-2 min | Make it configurable; keep camera backup out of the vault | Modified |
| AC-15 desktop footprint | 100k files, <= 1% CPU idle, <= 100 MB, scan <= 60 s | Walk+stat of 100k files took 0.3 s warm (measured, tmpfs); no published figure exists for any sync client at 100k files; Windows/NTFS with Defender is unmeasured | "Scan" = enumerate + stat, hashing runs later; state lives in SQLite, not RAM; the settings window is a separate process not counted. Needs a spike on each OS | Modified |
| AC-16 settings window | opens <= 1 s; applies <= 5 s | Native toolkits open in tens of ms; Tauri cold start on Windows and WebKitGTK is unmeasured | Measure on the weakest supported machine; pre-spawn a hidden window as the fallback | Kept |
| AC-17 native dialogs | OS dialog for any file/folder pick | Supported by the `rfd` crate on all three OSes | | Kept |
| AC-18 watcher | no echo loop; settle 2 s | Syncthing batches 10 s, Mutagen waits for stability; 2 s is aggressive but needed for AC-5 | Echo suppression keyed on OS file id, size, mtime and hash; temp file + atomic rename | Kept |
| AC-19 sign-in | all four providers | Per-provider operator cost: GitHub minutes; Google app must be "In production" (testing mode gives 7-day refresh tokens); Apple US$99/yr plus Services ID and key; Facebook needs Live mode, privacy-policy and data-deletion URLs | Reword to "each configured provider"; tests use a mock provider plus a manual release checklist | Modified |
| AC-20 abuse limit | > 10 failed sign-ins/min/IP rejected | With social-only login there are no passwords; per-IP limits break behind NAT and proxies; RFC 8628 requires limiting device-code guesses | Define "failed sign-in" (bad state/code, invalid token, wrong device code, wrong share password); add per-code, per-account and per-link limits | Modified |
| AC-21 revocation | stops <= 5 s | Trivial with opaque server-checked tokens; live sync streams must be cut actively | Test on the sync stream, not only on REST | Modified |
| AC-22 install time | <= 30 min | Credible only with one provider configured and a reverse proxy decision made | Reword accordingly | Modified |
| AC-23 signup control | (new) | Open social login on a self-hosted server lets any GitHub/Google user register | Default: invite or allowlist, with an admin role | Added |
| AC-24 scrub | (new) | Needed because ext4/XFS do not checksum data | A scheduled hash scrub reports any mismatch, throttled so it does not starve users | Added |
| AC-25 media sandbox | (new) | Crafted media is a known attack path (libwebp CVE-2023-4863, FFmpeg decoders) | Thumbnail and transcode workers run sandboxed: no network, CPU/memory/time limits | Added |
| AC-26 no-tray desktop | (new) | GNOME shows no tray by default; a Windows service cannot own a tray icon | The daemon syncs fully without a tray; Windows uses a logon task | Added |
| AC-27 account deletion | (new) | Apple, Facebook and GDPR require in-app deletion | Deleting an account removes its blobs, thumbnails, transcodes and shares | Added |

## Restrictions Assessment

| Restriction | Our Values | Researched Values | Cost/Timeline Impact | Status |
|-------------|-----------|-------------------|---------------------|--------|
| Server file storage | "plain tree on ext4" (hunch) | Immutable blobs named by ID (`blobs/<2hex>/<2hex>/<uuid>`) on ext4 or XFS, with the folder tree in the database. A path-mirroring tree ranks second | Moves and renames become one DB update. Operators cannot browse the data directory by real file names | Modified |
| Metadata database | Postgres by default | PostgreSQL 18 required: SQLite allows one writer on one host, which blocks adding workers | One more container in Compose | Modified |
| Sync protocol | not set | Blocklist over plain HTTPS, fixed 2 MiB blocks, 4 lanes, BLAKE3 hash, TLS 1.3 over TCP with HTTP/2, axum 0.8 on tokio | No HTTP/3, no content-defined chunking in v1 | Added |
| Change notification | not set | Held long-poll on a per-account cursor, SSE later if needed; no FCM/APNs in v1 | | Added |
| Auth architecture | social login only | Built-in OAuth/OIDC client in the Rust API (Dex has no Facebook or Apple connector; Keycloak needs 1-1.5 GB RAM). Identity key is (provider, id), never email. Server issues its own opaque sessions | Apple key signing is done by the server | Modified |
| Desktop login | device-code flow | Apple has no device flow; Google and Facebook restrict theirs. The server runs its own RFC 8628 device grant after normal browser login | | Modified |
| Vault crypto | opt-in E2EE | One shared Rust crypto core compiled native, to WASM (web) and via flutter_rust_bridge (mobile). XChaCha20-Poly1305 in chunks, per-file key, Argon2id (64 MiB, t=3). Web Crypto lacks XChaCha and Argon2 | Single owner per vault; sharing only by link with the key in the URL fragment | Added |
| Desktop UI | Rust UI, toolkit open | Headless daemon with tray (`tray-icon`, `ksni`); settings as an on-demand Tauri 2 + Svelte process with `rfd`, talking to the daemon over a local socket. Slint is the fallback and is GPLv3 or paid | Project license decision needed if Slint is chosen | Added |
| Media tooling | not set | libvips thumbnails (320 px WebP + 1440 px preview) in a sandboxed subprocess; `ffmpeg` as a subprocess, H.264 High 1080p AAC MP4; Postgres job queue with SKIP LOCKED; no Redis | HEIC decode has an unresolved HEVC patent question | Added |
| Mobile backup | Flutter + native code | Android: native WorkManager with a MediaStore trigger (Play rejects foreground-service backup). iOS: Swift PhotoKit background-upload extension (iOS 26.1+) plus foreground catch-up; the server must speak the IETF resumable-upload draft | Open: whether Apple allows a user-supplied server URL in that extension | Added |
| Windows autostart | service or logon task | Logon task only, because a service cannot show a tray | | Modified |
| Reverse proxy | not set | Request buffering must be off; body-size limits raised | Decide whether Compose ships one | Added |

## Key Findings

1. Your "plain tree on ext4" hunch is half right: blobs sit on ext4 as plain files, but named by ID with the folder tree in Postgres, because folder moves must be instant and one source of truth avoids drift (Nextcloud's file-plus-DB cache drifts; Immich and Dropbox use an ID-based tree).
2. Hashing and HTTP are not bottlenecks (BLAKE3 ~2.85 GB/s single-thread; a 1 Gbps link is 125 MB/s). Disk, proxy buffering and the 2 s settle time decide the targets.
3. AC-7 "0 silent corruption" cannot hold at rest on ext4/XFS. Restated as detect-and-report via scrub.
4. The four-provider social login is the largest operator burden: Apple costs US$99/yr, and Apple has no device flow, so the server brokers desktop login itself.
5. The 100 MiB vault cap excludes most phone video; camera backup must not go into the vault.
6. AC-11 is realistic only at a fast x264 preset on a CPU with real cores.
7. iOS background upload has no latency guarantee; AC-9 for iOS stays best effort.

## Sources

See the numbered source lists in `raw/storage_db.md` (51), `raw/sync_protocol.md`, `raw/media_pipeline.md` (60) and `raw/security_clients.md`. Claims marked [limited source] there were not confirmed by a primary page.
