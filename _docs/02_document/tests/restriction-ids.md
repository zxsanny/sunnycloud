# Restriction IDs

Stable IDs for the bullets of `_docs/00_problem/restrictions.md` (numbered in file order; restate the text when citing).


## Releases

- RESTRICT-01: v1.0: server, web (Svelte 5), Android, Linux and Windows desktop, GitHub and generic-OIDC sign-in (Google through OIDC), CPU transcoding.
- RESTRICT-02: v1.1: iOS, macOS desktop, Sign in with Apple, optional NVIDIA GPU encoding.
- RESTRICT-03: v1.2: end-to-end encrypted vault, released only after an external review.
- RESTRICT-04: Out of scope: Facebook sign-in, Intel/AMD/Apple GPU encoding (documented as experimental only).

## Technology

- RESTRICT-05: Rust for the API, background workers and desktop daemons. Svelte 5 for the web client. Flutter for Android and iOS, with native Kotlin and Swift where Flutter cannot reach a platform feature.
- RESTRICT-06: Desktop (Linux, Windows, later macOS): native Rust daemon with a tray icon and a small settings window built with Slint; Tauri 2 + Svelte is the fallback if Slint fails a spike. Settings use native OS dialogs for any file or folder selection and draw no custom file browser. Include and exclude rules are typed patterns.
- RESTRICT-07: Server storage: immutable blobs named by ID on XFS (documented default) or ext4 (`blobs/<2hex>/<2hex>/<id>`), with the folder tree and metadata in PostgreSQL 18. SQLite is excluded because it limits writers to one host. Names are unique per folder case-insensitively and stored case-preserving.
- RESTRICT-08: Blobs are encrypted at rest by the server with per-blob keys wrapped by an operator-supplied master key (envelope encryption: XChaCha20-Poly1305 STREAM in 64 KiB chunks so range reads work; AES-GCM is not used because it falls to about 0.12 GB/s without AES hardware). The master key is never stored with the data and must be backed up separately.
- RESTRICT-09: A block counts as received only after a batched fdatasync. Upload sessions reserve quota at start, are capped per account, and respect a free-space floor. The change feed cursor is `epoch.seq` per account; an unknown epoch or a seq below the purge horizon (tombstones kept 90 days) returns `resync_required`.
- RESTRICT-10: Replaced and deleted blobs are kept for a retention period (default 7 days, 0 to 90) through a garbage-collection queue written in the same transaction as the change. This is operator restore only; users get no trash. Account deletion is immediate.
- RESTRICT-11: Sync protocol: plain HTTPS over TCP (TLS 1.3, HTTP/2) with fixed 2 MiB blocks, at most 4 parallel lanes, BLAKE3 hashing, server-assigned commit order, single-request upload for files up to 2 MiB, a batch commit endpoint, and a held long-poll on the per-account cursor. Clients send the base revision of each change so stale edits become conflict copies. No HTTP/3 and no content-defined chunking. Server on axum and tokio with a hand-built hyper accept loop and large HTTP/2 windows. API documented with OpenAPI under `/api/v1`.
- RESTRICT-12: Dependency floors: tokio 1.54.1 or later (1.54.0 was yanked), h2 0.4.16 or later, bytes 1.11.1 or later, rustls 0.23.45 or later, libheif 1.22.0 or later. `cargo audit` and `cargo deny` are blocking CI steps.
- RESTRICT-13: Media: libvips thumbnails (`vipsthumbnail`) and `ffmpeg` as subprocesses. ffmpeg is built without `libfdk_aac` and without nonfree filters and uses the native AAC encoder; H.264 High 1080p MP4. A video is transcoded only when it is not already compliant. Workers split into a trusted supervisor (holds only the worker bearer secret: no database login, no blob mount and no master key; it claims jobs, fetches decrypted input and writes results through the API worker listener) and a decoder child with no credentials, no blob mount and no network (`network_mode: none`); ffmpeg reads a decrypted scratch file, not a pipe; ffmpeg protocol and format whitelists and `VIPS_BLOCK_UNTRUSTED=1` are mandatory, with a separate profile for HEIC. A Postgres `jobs` table with SKIP LOCKED is the queue; no Redis.
- RESTRICT-14: NVIDIA NVENC is opt-in and off by default [v1.1]; CPU is the acceptance baseline.
- RESTRICT-15: Mobile backup: Android uses native WorkManager with a MediaStore trigger and user-initiated data-transfer jobs for large first syncs. One upload implementation, in Rust, serves every mobile entry point (exposed through UniFFI; Kotlin and Swift own the handle). iOS uploads while the app is open plus system-scheduled background work; the iOS 26.1 upload extension is deferred.
- RESTRICT-16: Docker Compose is the install; it ships Caddy as the default reverse proxy with request buffering off and raised body limits.

## Authentication

- RESTRICT-17: Built-in OAuth/OIDC client in the Rust API; identity is (provider, provider user id), never email. Providers: GitHub and any generic OIDC provider in v1.0 (Google works through OIDC); Sign in with Apple in v1.1. No local passwords.
- RESTRICT-18: The first admin is created with a one-time setup token and the `sunnycloud admin` command. Later registration uses single-use invite tokens or an operator allowlist; an email address is only matched at invite redemption, and only if the provider verifies it.
- RESTRICT-19: The server issues its own opaque sessions with refresh rotation and a reuse grace window, plus its own RFC 8628 device grant for desktop daemons (Apple has no device flow). Device tokens are limited to syncing and are stored in the OS keychain, falling back on Linux to a user-only file with a logged warning.
- RESTRICT-20: A server with no internet access cannot authenticate users. Operators register their own provider apps.
- RESTRICT-21: The iOS app shows Google or GitHub sign-in only when the server also has Sign in with Apple enabled (App Store guideline 4.8).

## Encryption and vault [v1.2]

- RESTRICT-22: TLS for all traffic. Vault folders are end-to-end encrypted with a passphrase separate from the login and a one-time recovery key; losing both makes vault files unrecoverable.
- RESTRICT-23: Vault crypto is one shared Rust core compiled native, to WASM and through UniFFI: chunked XChaCha20-Poly1305 with a per-file key, Argon2id derivation (64 to 512 MiB, at least 3 passes, parallelism 1) with an authenticated header, associated data that binds the vault ID, file ID and format version (the parent is not bound, so moves and restores need no re-wrap; owner decision at plan Step 5), and a key-commitment value. A vault has a single owner; vault files cannot be shared by link in v1.2 (a revision gets a new file key on every edit, so a link would go stale). Vault files get no server thumbnails or transcoding.

## Sync model

- RESTRICT-24: Last commit wins; a stale edit is kept as a conflict copy; an edit beats a delete; no version history or user trash. The desktop client guards against mass deletes (missing or empty root, 200 or more deletes in a batch) and never hydrates or deletes cloud-evicted placeholder files.

## Platform

- RESTRICT-25: Desktop daemons run at login without UI: systemd user service (Linux), Windows logon task (a service cannot own a tray icon), LaunchAgent (macOS). GNOME shows no tray by default, so the daemon must work without one.
- RESTRICT-26: macOS: the user grants file access once in system settings when syncing protected locations.
- RESTRICT-27: iOS limits background work, so camera backup there is best effort.

## Hosting, licence and operations

- RESTRICT-28: The operator installs on their own infrastructure; hundreds of users per install; transcode workers on operator hardware. Streaming video is out of scope.
- RESTRICT-29: Licence: AGPL-3.0 for server, workers, desktop and apps, with an App Store exception file. The worker image ships ffmpeg with libx264 (GPL); whoever publishes it must also offer the corresponding source. Slint (GPL-3.0) is compatible. The licence file is added before the first outside contribution.
- RESTRICT-30: Builds on GitHub Actions with actions pinned by commit SHA and image digests pinned; fallback is git.azaion.com (Forgejo) with ci.azaion.com (Woodpecker). Primary remote: https://github.com/zxsanny/sunnycloud.git.
- RESTRICT-31: Backup and restore (database, blobs, master key) and release upgrades are documented and tested.

## Added after owner confirmation of gap defaults (IDs appended to keep earlier numbering stable)

- RESTRICT-32: Thumbnails and transcodes are encrypted at rest like every other blob. Job leases last 5 min with 3 attempts; upload sessions expire after 24 h and orphan blobs older than 48 h are swept. Batch commits are idempotent by a client-supplied commit ID. An interrupted upgrade rolls back its migration transaction and the server refuses to start until it is re-run. Provider calls time out after 10 s, HSTS max-age is one year, and the server recovers within 60 s of the database returning.
- RESTRICT-33: The Android device token is excluded from backups. The vault passphrase must have at least 12 characters and pass a strength check.
