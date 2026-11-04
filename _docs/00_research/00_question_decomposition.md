# Question Decomposition

## Original question
Design a self-hosted file, photo and video sync service (Rust API and workers, Svelte 5 web, Flutter mobile, native Rust desktop daemons) that meets `_docs/00_problem/` with maximum performance.

## Mode and rationale
Mode A, Phase 2 (initial research, no `solution_draft*.md` yet). Phase 1 (AC assessment) is confirmed and applied to `acceptance_criteria.md` and `restrictions.md`.

## Research Output Class
**Technical-component selection.** All gates apply: per-mode API verification (`02_fact_cards/mve_*.md`), component fit matrix (`06_component_fit_matrix.md`). Integration wire verification applies to the inbound OAuth callbacks (Google, Apple form_post, Facebook, GitHub) and is carried as a runtime gate in the fit matrix (mock provider in CI plus a manual release checklist).

## Question type
Decision Support (cost, risk and benefit per component) combined with Knowledge Organization for the sync-correctness and security landscape.

## Subject boundary
- **Population:** self-hosting operators (hundreds of users per install) and their users on web, Android, iOS, macOS, Windows, Linux.
- **Geography:** global; no regional restrictions assumed.
- **Timeframe:** components current as of 2026-11-02; Rust crate versions and OS features (for example iOS 26.1) as of that date.
- **Operating context:** the operator's own machines; Docker Compose install; transcode workers on operator hardware, optionally with GPUs; no cloud-only services.
- **Required interfaces:** HTTPS API documented with OpenAPI; OAuth providers Google, Apple, Facebook, GitHub; OS-native dialogs on desktop.
- **Non-functional envelope:** AC-1 to AC-28 in `acceptance_criteria.md` (500 users, 1 Gbps transfers, 5 s sync, 100k-file watcher at 100 MB / 1% CPU, vault E2EE, 5 s revocation).

## Project Constraint Matrix (hard disqualifiers)
| Constraint | Source | Disqualifies |
|-----------|--------|--------------|
| Rust for API, workers, desktop daemons | restrictions | Non-Rust server stacks |
| Svelte 5 web, Flutter mobile | restrictions | Other UI frameworks |
| No local passwords; social login only | restrictions | Password-based auth libraries as the primary path |
| Opt-in E2EE vault with a separate passphrase | restrictions | Designs where the server holds vault keys |
| Last write wins, no versions | restrictions | Version-vector or CRDT merge designs |
| Native OS dialogs, no custom file browser | AC-17 | Toolkits that force a custom file picker |
| Daemon works with no tray | AC-26 | Designs where the tray owns the sync loop |
| Docker Compose install by one operator in 30 min | AC-22 | Kubernetes-only or multi-service stacks needing Redis, Kafka, etc. |
| Workers on operator hardware, GPU optional | AC-11, AC-28 | Cloud-only transcoding services |
| Hundreds of users per install | AC-1 | Sharding-first architectures |

## Sub-questions
1. How do existing sync systems store blobs and metadata, and what fits 500 users and 50 GiB files? (storage_db)
2. Which sync protocol and transport meet AC-2, AC-3, AC-5? (sync_protocol)
3. How should the desktop watcher behave to meet AC-15 and AC-18 on three OSes? (sync_protocol, mve_desktop)
4. How are thumbnails and transcodes produced with CPU and GPU workers, and safely? (media_pipeline, mve_server_media)
5. How does camera backup work on Android and iOS? (media_pipeline, mve_crypto_auth_mobile)
6. How should social login, sessions and desktop device login work for self-hosters? (security_clients, mve_crypto_auth_mobile)
7. How should the E2EE vault be designed across Rust, web and Flutter? (security_clients, mve_crypto_auth_mobile)
8. Which desktop tray and settings approach fits the memory and startup targets? (security_clients, mve_desktop)
9. What security requirements are missing? (security_clients)

## Perspectives used
Implementer (pitfalls and crate limits), Practitioner (Immich, Nextcloud, Syncthing, Seafile, Dropbox experience reports), Contrarian (HTTP/3, Kubernetes, SQLite, E2EE breaks), Domain expert (FAST'08 corruption study, WWW'24 QUIC paper, ETH E2EE attacks), Business/operator (cost of Apple and Facebook registration, Slint licence).

## Component Option Search Plan (summary)
Each area covered at least baseline, established, open-source, SOTA/adjacent, defer/no-build and known-bad options. The raw files hold the option tables and queries; one row per area:

| Area | Options covered | Report |
|------|-----------------|--------|
| Blob storage | path-mirror tree, ID blobs, chunked CAS, S3-compatible (Garage, MinIO, SeaweedFS), ZFS/btrfs | raw/storage_db.md |
| Metadata DB | PostgreSQL, SQLite+Litestream, embedded KV (redb, sled), FoundationDB | raw/storage_db.md |
| Transfer protocol | tus, S3 multipart, Dropbox blocklist, Syncthing BEP, FastCDC delta | raw/sync_protocol.md |
| Transport | HTTP/1.1, HTTP/2, HTTP/3, gRPC | raw/sync_protocol.md |
| Change notification | polling, long-poll, SSE, WebSocket, push | raw/sync_protocol.md |
| Thumbnails | libvips, `image` crate, ImageMagick, turbojpeg | raw/media_pipeline.md |
| Video | ffmpeg CLI CPU, ffmpeg GPU encoders, ffmpeg-next, AV1 | raw/media_pipeline.md, mve_server_media |
| Job queue | Postgres SKIP LOCKED, pgmq, Redis | raw/media_pipeline.md |
| Auth | built-in OAuth, Dex, Keycloak, Authentik | raw/security_clients.md |
| Vault crypto | XChaCha20-Poly1305 STREAM, AES-GCM, libsodium secretstream, age | raw/security_clients.md |
| Desktop UI | Tauri 2, egui, Slint, iced, native split | raw/security_clients.md |
| Mobile backup | WorkManager, foreground service, iOS BGTask, iOS 26.1 upload extension | raw/media_pipeline.md |

## Completeness audit
Probes covered: storage, transfer, sync correctness, notification, media, mobile, auth, crypto, desktop UI, security. Added from the audit: signup control (AC-23), scrub (AC-24), media sandbox (AC-25), no-tray desktop (AC-26), account deletion (AC-27), GPU encoding (AC-28). Not covered by research and carried as open items: observability and metrics stack, backup procedure for operators, upgrade/migration path, quota enforcement design, admin UI scope, internationalisation, accessibility.

## Timeliness assessment
Novelty sensitivity: **High** for Rust crate versions and mobile OS APIs (iOS 26.1, Android 15), **Medium** for protocols and filesystems. Sources are dated 2023-2026 with versions annotated in the reports; claims that rest on secondary sources are marked [limited source].
