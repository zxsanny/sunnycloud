# Reasoning Chain

Fact references use `<report>#<fact or source number>` from the files in `raw/` and `02_fact_cards/`.

## Dimension 1: Where files and the tree live
**Facts.** Dropbox's sync engine needs stable file IDs and atomic moves (sync_protocol#1). oCIS's path-tree driver is still experimental and Nextcloud's file-plus-DB cache drifts and bloats (storage_db). Immich keeps the tree in Postgres (storage_db). ext4/XFS handle directory fan-out of `<2hex>/<2hex>` (storage_db).
**Comparison.** A mirrored tree makes the filesystem a second source of truth and turns a folder move into N renames plus N DB updates; the ID-based design makes a move one row update.
**Conclusion.** ID-named immutable blobs on ext4/XFS plus the tree in PostgreSQL. The cost is that operators cannot browse the data directory by names.
**Confidence.** High on the design; Medium that dedup is unnecessary (benefit for photos and video is [limited source]).

## Dimension 2: Metadata database
**Facts.** SQLite WAL allows one writer and shared memory on one host (storage_db); AC-11 requires adding workers; sqlx 0.9 supports Postgres, PgListener, SKIP LOCKED (mve_server_media #2). Postgres sequences are not commit-ordered (sync_protocol#51).
**Conclusion.** PostgreSQL 18. The change feed uses a per-account counter incremented inside the write transaction, not a global sequence. LISTEN/NOTIFY only wakes pollers; the cursor table is the source of truth.
**Confidence.** High.

## Dimension 3: Transfer protocol and transport
**Facts.** Hashing runs at 2-13 GB/s versus a 125 MB/s link (sync_protocol#29). HTTP/3 loses up to 45% throughput on fast links (sync_protocol#18). 2 MiB blocks x 4 lanes = 8 MiB worst-case resend (AC-3). Hyper's default HTTP/2 window caps a 20 ms link near 420 Mbit/s (mve_server_media #1).
**Conclusion.** Blocklist protocol over HTTPS/HTTP-2, explicit large window settings via a hand-built hyper-util accept loop, reqwest with explicit windows on clients, BLAKE3 per block and per file. Reverse proxy buffering must be off.
**Confidence.** High on shape; Medium on AC-2 until measured.

## Dimension 4: Notification and ordering
**Facts.** The AC-5 budget is 2.4-3.6 s with 2 s of settle time (sync_protocol); 30 s polling cannot meet it; PgListener loses messages across reconnect (mve_server_media #2); self-hosted iOS push needs a central relay (sync_protocol#49).
**Conclusion.** Held long-poll on the per-account cursor (`GET /api/changes?cursor=N&wait=30`); SSE later if request churn appears. No FCM/APNs in v1. Order by server commit, never client mtime.
**Confidence.** High.

## Dimension 5: Desktop client
**Facts.** notify gives hints only; Windows overflow is silent in 8.2.1; the debouncer lacks "unchanged for 2 s" (mve_desktop #1). tray-icon with ksni builds without a tray host (mve_desktop #2). rusqlite uses 7 MB per 100k rows versus 22 MB for redb (mve_desktop #7). Tauri start-up is unmeasured (mve_desktop #3).
**Conclusion.** Daemon owns sync, state in SQLite, own settle and echo layer, safety rescans; tray in the daemon; Tauri settings on demand, Slint fallback.
**Confidence.** Medium: AC-15 and AC-16 depend on per-OS spikes.

## Dimension 6: Media pipeline
**Facts.** Resize costs 6-16 ms (media_pipeline); x264 1080p runs 26-61 fps (media_pipeline#33); NVENC runs here with 12 concurrent sessions (mve_server_media #6); the `libvips` crate decodes in-process (mve_server_media #5); `VIPS_BLOCK_UNTRUSTED` blocks HEIC.
**Conclusion.** Sandboxed workers invoking `vipsthumbnail` and `ffmpeg` as subprocesses; CPU x264 and NVENC for v1; VAAPI/QSV/VideoToolbox experimental until tested on hardware; HEIC uses a second worker profile.
**Confidence.** High for CPU and NVENC; Low for other GPUs.

## Dimension 7: Auth
**Facts.** Dex lacks Facebook and Apple; Keycloak needs 1-1.5 GB RAM (security_clients). Apple has no device flow. Apple needs a hand-written client-secret JWT (mve_crypto_auth_mobile #5). App Store 4.8 requires Sign in with Apple when other social logins are offered (mve_crypto_auth_mobile #9).
**Conclusion.** Built-in OAuth client, opaque hashed sessions, server-issued device grant. iOS provider rule is a user decision.
**Confidence.** High.

## Dimension 8: Vault
**Facts.** Web Crypto lacks XChaCha20 and Argon2 (security_clients). `aead::stream` was removed in aead 0.6; `aead-stream` 0.6.0 replaces it with no stated audit; tests reject truncation, reorder and bit flips (mve_crypto_auth_mobile #1). ETH papers show independent implementations break E2EE (security_clients).
**Conclusion.** One Rust core built native, to WASM and via flutter_rust_bridge; XChaCha20-Poly1305 STREAM with Argon2id. An external crypto review is required before the vault is released.
**Confidence.** Medium (unaudited stream crate).

## Dimension 9: Mobile backup
**Facts.** Google Play rejects foreground-service backup; WorkManager with MediaStore trigger is compliant (mve_crypto_auth_mobile #7). The iOS 26.1 extension fixes the destination in Info.plist and needs a 104 response that hyper cannot send (mve_crypto_auth_mobile #8).
**Conclusion.** Android native WorkManager. iOS v1 uses foreground catch-up plus system-scheduled background work; the 26.1 extension is deferred behind a device spike.
**Confidence.** High for Android; Medium for iOS.
