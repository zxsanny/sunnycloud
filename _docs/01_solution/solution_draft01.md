# Solution Draft 01

## Product Solution Description

sunnycloud is a self-hosted sync service installed by an operator with Docker Compose. A Rust API server holds file metadata in PostgreSQL 18 and file contents as immutable blobs on local disk (ext4 or XFS). Rust workers on the operator's machines make thumbnails and transcode video, using NVIDIA GPUs when present. Clients sync through one block-based HTTPS protocol: a Svelte 5 web app, Flutter apps for Android and iOS, and a native Rust desktop daemon with tray and settings window for macOS, Windows and Linux. Opt-in vault folders are encrypted on the client by one shared Rust crypto core.

```
 Web (Svelte 5 + WASM vault)   Flutter mobile (+frb vault)   Desktop daemon + tray (+ Tauri settings)
              \                         |                          /
               +------- HTTPS / HTTP-2, blocklist protocol, long-poll cursor -------+
                                        |
                              API server (axum/hyper, Rust)
                       sessions, OAuth, device grant, shares, quotas
                          |                              |
                 PostgreSQL 18                    blob store (ext4/XFS)
          tree, blocks, cursor, jobs, users        blobs/<2hex>/<2hex>/<uuid>
                          |
              workers (Rust, sandboxed containers)
        vipsthumbnail | ffmpeg (x264 CPU, NVENC GPU)
```

## Existing/Competitor Solutions Analysis

| System | Takeaway for sunnycloud |
|--------|-------------------------|
| Dropbox (Nucleus) | Stable file IDs, atomic moves, blocklist with "need blocks" commit: adopted. |
| Syncthing | Block exchange and conflict rules; 10 s watcher batching is too slow for AC-5; QUIC slower than TCP on LAN. |
| Nextcloud | Chunked uploads failed at the final assembly step on a 43 GB file; file-plus-DB cache drifts: avoid assembly copy and mirrored trees. |
| Seafile | Content-defined chunking and dedup: deferred (benefit for photos and video unproven). |
| Immich | Postgres as the source of truth, queue-driven media jobs, mobile backup pain points: adopted pattern. |
| oCIS | Path-tree storage driver is still experimental: reinforces the ID-based tree. |
| Cryptomator, Ente, Proton Drive | Chunked AEAD vaults with client-side keys: adopted pattern. |

## Architecture

All component areas below were checked against the Project Constraint Matrix; the sub-matrix for each is in `_docs/00_research/06_component_fit_matrix.md` and the per-mode API evidence in `_docs/00_research/02_fact_cards/mve_*.md`. "Selected + gate" means the API is proven and a named spike (S1-S11 in the matrix) must still pass.

### Component: Storage and metadata

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| ID-named blobs + tree in Postgres | ext4/XFS, PostgreSQL 18, sqlx 0.9 | Pre-sized staging file per upload, finished by rename, then DB commit; `blobs/<2hex>/<2hex>/<uuid>`; per-account change counter inside the write transaction | O(1) folder moves, one source of truth, no assembly copy, easy backup | Operators cannot browse files by name; no data checksums on ext4/XFS | Selected + gate (S6) |
| Path-mirroring tree | ext4 | names as paths | Browsable | Two sources of truth, slow moves, drift | Rejected for v1 (fallback) |
| Chunked CAS with dedup | custom | CDC chunks | Dedup, delta | Complexity, little benefit for media | Rejected for v1 |
| S3-compatible backend | Garage, SeaweedFS | object store | Scale-out | Extra service; MinIO community edition in maintenance | Rejected for v1 |
| SQLite / redb | | | Simple | One writer, one host | Rejected |

Integrity: BLAKE3 per block and per file in flight; a throttled scrub job re-hashes blobs (AC-24); ECC RAM and a ZFS/btrfs mirror are recommended in the install docs. Disk-full and quota handling are enforced before accepting a commit.

### Component: Sync protocol and server

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| Blocklist protocol over HTTPS | axum 0.8.9, hyper 1.12, hyper-util, tokio, BLAKE3 | Client sends blocklist (2 MiB blocks); server returns missing blocks; up to 4 lanes; commit publishes atomically; server-assigned commit order; long-poll `GET /api/changes?cursor=N&wait=30` | Resume loses at most 8 MiB; unchanged blocks are never re-sent; ordering ignores client clocks | Needs a hand-built accept loop with large H2 windows and per-route body limits; no 1xx responses from hyper | Selected + gate (S1) |
| tus / S3-multipart style | | | Standard | Weaker fit for block reuse | Rejected |
| HTTP/3 | quinn, h3 | | | Up to 45% lower throughput on fast links; `h3` is "very experimental" | Rejected |

API is documented with OpenAPI under `/api/...`. Reverse proxy must have request buffering off and raised body limits.

### Component: Media pipeline

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| Thumbnails | `vipsthumbnail` CLI (libvips 8.18.7) in a sandboxed worker | 320 px WebP and 1440 px preview; second profile for HEIC | Fast shrink-on-load, process isolation | HEIC has no shrink-on-load; `VIPS_BLOCK_UNTRUSTED` blocks HEIC | Selected + gate (S4) |
| Video, CPU | ffmpeg + libx264 | H.264 High 1080p AAC MP4 faststart, preset veryfast/fast, CRF 23 | Works everywhere | 4 physical cores needed for AC-11 | Selected + gate (S5) |
| Video, NVIDIA | ffmpeg + NVENC | same output; test-encode probe at start; CPU fallback | About 3x faster | Session limits on consumer GPUs; `RLIMIT_AS` < 4 GiB breaks it | Selected + gate (S5, S10) |
| Video, Intel/AMD/Apple | VAAPI, QSV, VideoToolbox | same | Existing GPUs | Unverified; no test hardware | Experimental only |
| Queue | Postgres `jobs` table, SKIP LOCKED, leases, unique (kind, content hash, params version), separate thumbnail and transcode lanes | | No extra service | Hand-written | Selected |
| Sandbox | Compose: `network_mode: none`, cgroup limits, Unix-socket access to Postgres | | Reproducible | macOS GPU workers have no sandbox path | Selected + gate (S10) |

Vault files get no server media processing; clients make vault thumbnails.

### Component: Identity and sessions

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| Built-in OAuth/OIDC client | openidconnect 4.1.0, oauth2 5.0.0, jsonwebtoken | Google OIDC; Apple OIDC with no scopes and no PKCE (hand-written ES256 client secret from the operator's .p8); GitHub and Facebook OAuth2 plus profile calls; identity key (provider, user id) | No extra identity service | Facebook PKCE unverified; crates partly old | Selected + gate (S8) |
| Sessions | opaque tokens, SHA-256 hashed at rest, refresh rotation with reuse detection | | Revocation within 5 s; live streams cut by wake-up | Hand-written | Selected |
| Desktop login | hand-written RFC 8628 device grant (8-character code, 5 attempts) | | Works for Apple, which has no device flow | Hand-written | Selected |
| Registration | invite or allowlist, admin role | | Prevents open signup | Admin UI scope open | Selected |

iOS shows Google, Facebook or GitHub only when the server also enables Apple.

### Component: Vault (end-to-end encryption)

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| Shared Rust core | chacha20poly1305 0.11.0 + aead-stream 0.6.0, argon2 0.6.0, wasm-bindgen 0.2.130, flutter_rust_bridge 2.13.0 | XChaCha20-Poly1305 STREAM (BE32, 19-byte prefix), per-file key; Argon2id 64 MiB, t=3, p=1; header holds format version, chunk size, prefix, KDF parameters; recovery key wraps the vault key | One implementation on web, Android, iOS and desktop | `aead-stream` has no stated audit; Cargokit archived | Selected + gate (S7, S11) |
| libsodium secretstream | | same shape | Audited | Extra builds for WASM and Dart | Fallback |

Single-owner vault; link sharing carries the key in the URL fragment; files over 100 MiB (configurable) are rejected; no camera backup into the vault. An external review of the construction is required before release.

### Component: Desktop client

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| Daemon | Rust, notify 8.2.1 + debouncer-full 0.7.0, rusqlite 0.41.1 (bundled, WAL), reqwest 0.13.5 (rustls, h2, explicit windows) | Watcher gives hints; our code does the 2 s settle, echo suppression keyed on file ID/size/mtime/hash, rename stitching and periodic safety rescans; scan = enumerate + stat, hashing later | Cross-OS, small footprint | Windows overflow is silent in 8.2.1 | Selected + gate (S2) |
| Tray | tray-icon 0.26.1 with ksni feature | tray in the daemon; daemon works with no tray | One crate for all three OSes | macOS needs the main thread | Selected |
| Settings | Tauri 2.12.5 + Svelte, on demand, JSON-RPC over a local socket; rfd 0.17.2 via tauri-plugin-dialog for native pickers | Separate process, not counted in AC-15 | Reuses Svelte | Start-up unmeasured | Experimental only (S3); Slint fallback |
| Autostart | systemd user unit, launchd agent (SMAppService), Task Scheduler logon task | | Native per OS | Signing needed on macOS | Selected |
| Token | OS keychain; Linux file with mode 0600 and a logged warning when no Secret Service | | | Weaker on bare Linux | Selected |

### Component: Mobile

| Solution | Tools | Pinned mode | Advantages | Limitations | Fit |
|----------|-------|-------------|-----------|-------------|-----|
| Android | Flutter UI, native Kotlin WorkManager with MediaStore content-URI trigger, resumable block upload | unmetered network, battery-exemption prompt | Play-compliant | No documented latency; needs device test (S9) | Selected + gate |
| iOS | Flutter UI, foreground catch-up on app open, system-scheduled background work | best effort, no time promise | Works with any server address | Slower than Android | Selected |
| iOS 26.1 upload extension | Swift PhotoKit extension | | True background upload | Fixed Info.plist destination; needs a 104 response | Experimental only (deferred) |
| Shared core | flutter_rust_bridge 2.13.0 | Android and iOS only | | NDK r28+ and 16 KB alignment check in CI | Selected + gate |

## Operator and delivery notes
- Docker Compose services: API, Postgres 18, thumbnail worker, transcode worker (optional GPU), optional reverse proxy. CI runs on GitHub Actions with a PG 18 job; fallback is git.azaion.com with Woodpecker.
- Project licence: GPL-compatible because the worker image ships libx264; GPL-3.0 versus AGPL-3.0 is chosen before the first public release. Slint is usable as fallback.
- Desktop signing and packaging (Apple Developer account, Windows signing, deb/rpm/AppImage/flatpak) are decided in the Deploy step.
- Not yet covered by research and carried forward: observability stack, operator backup and upgrade procedure, quota design, admin UI scope, internationalisation, accessibility.

## Testing Strategy

### Integration / Functional Tests
- Upload 5 GiB, kill the connection at 50%, resume: final hash equals, at most 10 MiB re-sent (AC-3, AC-4, AC-7).
- Two devices online: edit on A, measure appearance on B (AC-5); concurrent offline edits end with the later commit (AC-8).
- Revoke a device mid-stream: sync stops within 5 s (AC-21).
- Vault: server bytes differ from plaintext; tampered, truncated, reordered and wrong-vault chunks rejected; wrong passphrase fails (AC-13).
- Share links: open, revoke within 5 s, expiry, uniform 404 for missing/expired/revoked (AC-12).
- Account deletion removes blobs, thumbnails, transcodes, shares and sessions (AC-27).
- Mock OAuth provider in CI for all four providers, plus a manual per-provider release checklist (AC-19, AC-20, AC-23).
- Tricky names: case, NFC/NFD, reserved names, 300-character paths across OSes.
- Desktop: no echo upload of downloaded files; 2 s settle on a growing file; works with no tray (AC-18, AC-26).

### Non-Functional Tests
- S1 throughput at 1 Gbps with proxy buffering off (AC-2); S6 listing 10,000 entries (AC-6).
- S2 watcher with 100,000 files per OS (AC-15); S3 settings window cold start (AC-16).
- S4 thumbnail timing including HEIC, with a reserved lane under bulk import (AC-10); S5 transcode on CPU and NVENC reference workers (AC-11, AC-28).
- Security: path traversal, IDOR across users, share-token enumeration, upload abuse limits, hostile media in the sandbox, rate limits on defined failures (AC-20, AC-25), scrub detects a flipped bit (AC-24).

## References
Numbered source lists are in `_docs/00_research/raw/` (`storage_db.md`, `sync_protocol.md`, `media_pipeline.md`, `security_clients.md`); API verification sources are in `_docs/00_research/02_fact_cards/mve_*.md`. Assessment: `_docs/00_research/00_ac_assessment.md`. Fit matrix: `_docs/00_research/06_component_fit_matrix.md`.

## Related Artifacts
- Tech stack evaluation: not produced (Phase 3 skipped; stack choices are in the component tables above).
- Security analysis: not produced (Phase 4 skipped); missing security requirements are listed in `raw/security_clients.md` § Missing security requirements and reflected in AC-20 to AC-27.
