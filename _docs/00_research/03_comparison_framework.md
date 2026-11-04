# Comparison Framework

## Selected Framework Type
Decision Support with the exact-fit dimensions for component decisions.

## Selected Dimensions
1. Option family (simple baseline, established production, open-source, commercial, SOTA, adjacent, no-build, known bad)
2. Required inputs/outputs and ownership boundaries
3. Operating context and lifecycle fit (operator hardware, Docker Compose, 500 users)
4. Non-functional envelope fit (AC-1..28)
5. Implementation assumptions and hard disqualifiers
6. Evidence quality and source tier
7. Selection status (Selected, Rejected, Experimental only, Needs user decision)
8. Cost and operator burden
9. Security surface

## Initial Population (decisions, lead vs main alternatives)

| Area | Lead | Alternatives considered | Deciding dimension | Basis |
|------|------|------------------------|--------------------|-------|
| Blob storage | ID-named blobs on ext4/XFS, tree in Postgres | path-mirror tree; chunked CAS with dedup; S3 backend (Garage, SeaweedFS); ZFS/btrfs | Non-functional (O(1) moves, one source of truth), operator burden | storage_db F-cards on Dropbox, oCIS posixfs, Nextcloud drift, Immich |
| Metadata DB | PostgreSQL 18 | SQLite+Litestream; redb/sled; FoundationDB | Operating context (multiple writers: API + workers) | storage_db, mve_server_media #2 |
| Transfer protocol | Dropbox-style blocklist over HTTPS, 2 MiB blocks, 4 lanes | tus; S3 multipart; Syncthing BEP; FastCDC delta | AC-3 (re-send <= 10 MiB), AC-4 | sync_protocol |
| Transport | TLS 1.3 + HTTP/2 over TCP | HTTP/3; HTTP/1.1 per lane; gRPC | Non-functional (AC-2); HTTP/3 up to 45% slower on fast links | sync_protocol F-cards 18-19 |
| Hash | BLAKE3 | SHA-256 | Throughput; tree mode | sync_protocol, mve_server_media #3 |
| Server framework | axum 0.8 on hyper 1.12 + tokio | actix-web | Ecosystem fit; benchmarks inconclusive | sync_protocol 26 |
| Change feed | per-account commit counter + held long-poll | SSE; WebSocket; FCM/APNs; LISTEN/NOTIFY alone | AC-5, AC-21; NOTIFY loses messages across reconnect | sync_protocol, mve_server_media #2 |
| Job queue | hand-written SKIP LOCKED table | apalis, sqlxmq, pgmq, Redis | Dependency maturity; no extra service | mve_server_media #4 |
| Thumbnails | libvips via `vipsthumbnail` CLI, sandboxed | libvips crate; `image` crate; ImageMagick | Security isolation (AC-25) | mve_server_media #5 |
| Video | ffmpeg CLI; libx264 CPU, NVENC GPU | ffmpeg-next; AV1; VAAPI/QSV/VideoToolbox (unverified) | AC-11, AC-28; maintenance status | mve_server_media #6 |
| Worker sandbox | Compose container flags | bubblewrap; Landlock; nsjail | Reproducible install | mve_server_media #7 |
| Auth | built-in OAuth/OIDC client | Dex; Keycloak; Authentik | Provider coverage (Facebook/Apple), RAM | security_clients, mve_crypto_auth_mobile #5 |
| Desktop login | server-run RFC 8628 device grant | provider device flows; loopback | Apple has no device flow | security_clients |
| Vault crypto | XChaCha20-Poly1305 STREAM (`aead-stream`), Argon2id | AES-GCM; libsodium secretstream; age | Web Crypto lacks XChaCha; one Rust core | mve_crypto_auth_mobile #1-3 |
| Mobile glue | flutter_rust_bridge 2.13 (Android/iOS) | separate Dart crypto | Single implementation (ETH papers) | mve_crypto_auth_mobile #4 |
| Desktop watcher | notify 8.2 + our settle/echo layer | polling; fanotify | Cross-OS coverage | mve_desktop #1 |
| Desktop tray | tray-icon (ksni feature) | egui, Slint, iced | Works with no tray host | mve_desktop #2 |
| Settings UI | Tauri 2 + Svelte, on demand | Slint; egui | Reuse Svelte; AC-16 unproven | mve_desktop #3 |
| Client state | rusqlite bundled WAL | redb | Footprint (7 MB vs 22 MB per 100k rows, spike) | mve_desktop #7 |
| Android backup | native Kotlin WorkManager + MediaStore trigger | foreground service | Play policy rejects it | mve_crypto_auth_mobile #7 |
| iOS backup | foreground catch-up + BGTask/background URLSession | iOS 26.1 upload extension | Open blockers (see matrix) | mve_crypto_auth_mobile #8 |
