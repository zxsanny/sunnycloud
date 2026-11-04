# Component Fit Matrix

Status key: **Selected**, **Selected + runtime gate** (API capability proven, a quality measurement is still required), **Experimental only**, **Rejected**, **Needs user decision**.

The per-candidate Restrictions × Candidate-Modes sub-matrices are the "Restriction/AC binding" tables inside each MVE block (they walk the AC and restriction lines that apply and mark the rest N/A). Their `Verify` cells are runtime measurements, listed in the Runtime gates section; none is an unresolved API-capability question.

## Top-level matrix

| Component Area | Candidate | Pinned Mode/Config | Option Family | Role | API Capability Evidence | Mismatches / Disqualifiers | Status | Rationale |
|---|---|---|---|---|---|---|---|---|
| HTTP server | axum 0.8.9 + hyper 1.12.0 + hyper-util | streaming request bodies to file, hand-built accept loop with large H2 windows | Established, OSS | API and transfer endpoints | mve_server_media #1 | default H2 windows cap a 20 ms link near 420 Mbit/s; no 1xx responses; shutdown has no timeout | Selected + runtime gate (AC-2 test) | Window settings and per-route body limits are mandatory |
| DB access | sqlx 0.9.0 + PostgreSQL 18 | runtime SQL, migrations, PgListener, SKIP LOCKED | Established, OSS | metadata, queue, change feed | mve_server_media #2 | CI upstream tests PG 13/17 only; PgListener loses messages on reconnect | Selected + runtime gate (PG 18 CI job) | Cursor table stays the source of truth |
| Hashing | blake3 1.8.7 | streaming `Hasher::update` per block, `rayon` | Established, OSS | block and file hash | mve_server_media #3 | none | Selected | Bao deferred |
| Job queue | hand-written SKIP LOCKED table | leases, unique (kind, hash, params_version) | Simple baseline | thumbnail/transcode jobs | mve_server_media #4 | apalis-postgres 3 weeks old (Experimental only); sqlxmq, pgmq Rejected (need sqlx 0.8) | Selected | No extra service |
| Thumbnails | libvips 8.18.7 via `vipsthumbnail` CLI | subprocess in sandbox, JPEG/WebP; HEIC second profile | Established, OSS | thumbnails | mve_server_media #5 | `libvips` crate Rejected for workers; BLOCK_UNTRUSTED blocks HEIC; no HEIC shrink-on-load | Selected + runtime gate (AC-10 HEIC timing) | |
| Video | ffmpeg CLI, libx264 (CPU) and NVENC | H.264 High 1080p AAC MP4 faststart | Established, OSS | transcode | mve_server_media #6 | x264 build is GPL; `RLIMIT_AS` < 4 GiB breaks NVENC | Selected + runtime gate (AC-11) | |
| Video, other GPUs | VAAPI, QSV, VideoToolbox | same, vendor encoders | Established, OSS | transcode | mve_server_media #6 | commands unverified; no hardware here; `-encoders` lists unusable ones | Experimental only | Needs test hardware; probe-encode at worker start |
| Worker sandbox | Compose flags (`network_mode: none`, cgroup limits) | Linux containers | Established | AC-25 | mve_server_media #7 | bubblewrap fails in default Docker; no macOS GPU sandbox path | Selected + runtime gate (GPU in no-network container) | nsjail Rejected, bubblewrap/Landlock Experimental |
| Vault AEAD | chacha20poly1305 0.11.0 + aead-stream 0.6.0 | XChaCha20Poly1305, BE32 STREAM, 19-byte prefix | Established, OSS | vault chunks | mve_crypto_auth_mobile #1 | `aead::stream` removed; aead-stream has no stated audit | Selected + runtime gate (external crypto review before release) | libsodium secretstream is the audited fallback |
| KDF | argon2 0.6.0 | Argon2id m=64 MiB, t=3, p=1 | Established, OSS | vault key wrap | mve_crypto_auth_mobile #2 | p=1 deviates from RFC 9106 p=4; wasm needs getrandom feature off | Selected + runtime gate (phone/Safari timing) | |
| Web crypto glue | wasm-bindgen 0.2.130 + wasm-pack 0.15 | `--target web`, module Worker, single thread | Established, OSS | vault in browser | mve_crypto_auth_mobile #3 | docs name only webpack as compatible bundler | Selected + runtime gate (Vite + SvelteKit 100 MiB spike) | |
| Mobile glue | flutter_rust_bridge 2.13.0 | Android and iOS only | Established, OSS | shared Rust core | mve_crypto_auth_mobile #4 | Cargokit archived; Play needs 16 KB page alignment from 2027-02-01 | Selected + runtime gate (NDK r28+, alignment check in CI) | |
| OAuth clients | openidconnect 4.1.0 + oauth2 5.0.0 | Google OIDC, Apple OIDC no scope/no PKCE, GitHub/Facebook OAuth2 | Established, OSS | sign-in | mve_crypto_auth_mobile #5 | Apple secret JWT hand-written; Facebook PKCE unverified; oauth2 near 2 years old | Selected + runtime gate (live provider flows) | |
| Device grant | hand-written RFC 8628 server | SHA-256 token hashes | Simple baseline | desktop login | mve_crypto_auth_mobile #6 | no maintained crate | Selected | `oauth-as` Rejected |
| Android backup | native WorkManager + MediaStore trigger | unmetered, chunked resumable upload | Established | AC-9 | mve_crypto_auth_mobile #7 | no documented latency | Selected + runtime gate (device measurement) | `photo_manager` Experimental only |
| iOS background upload | iOS 26.1 upload extension | Swift PhotoKit extension | Vendor | AC-9 iOS | mve_crypto_auth_mobile #8 | destination is a build-time Info.plist string; needs 104 response hyper cannot send | Experimental only (deferred) | Owner decision D1: v1 uses foreground + system-scheduled upload; extension revisited after a device test |
| iOS sign-in rule | App Store 4.8 | Apple required alongside other providers | Vendor policy | AC-19 | mve_crypto_auth_mobile #9 | conflicts with "any subset" | Selected | Owner decision D2: iOS shows other providers only when Apple is enabled on the server |
| File watcher | notify 8.2.1 + debouncer-full 0.7.0 | recursive; hints only | Established, OSS | desktop | mve_desktop #1 | silent Windows overflow; no 2 s settle | Selected + runtime gate (per-OS spike) | We write settle, echo, rescan |
| Tray | tray-icon 0.26.1 (ksni feature) | in daemon | Established, OSS | tray | mve_desktop #2 | macOS needs main thread | Selected | |
| Settings UI | Tauri 2.12.5 + Svelte | on-demand process, JSON-RPC socket | Established, OSS | settings | mve_desktop #3 | no official start-up/RAM figures | Experimental only (AC-16 spike) | Slint is the fallback (GPLv3/paid) |
| Native dialogs | rfd 0.17.2 via tauri-plugin-dialog | inside the settings process | Established, OSS | AC-17 | mve_desktop #4 | standalone in daemon Rejected | Selected | |
| Token store | keyring 4.2.0 | Keychain, Credential Manager, Secret Service | Established, OSS | device token | mve_desktop #5 | no durable headless-Linux fallback | Selected | Owner decision D4: 0600 token file with a warning when no Secret Service exists |
| Autostart | systemd user unit, launchd agent, Task Scheduler logon task | per OS | Established | AC-26 | mve_desktop #6 | `auto-launch` Rejected (Run key) | Selected | |
| Client state | rusqlite 0.41.1 bundled WAL | local file index | Established, OSS | desktop | mve_desktop #7 | redb Rejected | Selected | |
| HTTP client | reqwest 0.13.5 (rustls, h2) | explicit windows, 4 lanes | Established, OSS | transfers | mve_desktop #8 | one shared TCP vs 4 connections unresolved | Selected + runtime gate (AC-2 spike) | |
| Packaging/signing | per OS | Developer ID + notarization, Windows signing, deb/rpm/AppImage | Vendor | distribution | mve_desktop #9 | costs and account choices open | **Needs user decision** (deferred to deploy step) | Not blocking the draft |
| Licence | libx264 in the worker image | GPL | Policy | distribution | mve_server_media #6 | GPL obligations for the shipped image | Selected | Owner decision D3: ship ffmpeg with libx264 and license the project GPL-compatible (GPL-3.0 or AGPL-3.0 chosen before first public release) |

## Runtime gates (measurements carried into the plan as spikes)
S1 AC-2 throughput with large H2 windows and proxy buffering off. S2 AC-15 100k-file watcher per OS (including Windows Defender). S3 AC-16 Tauri cold start on the weakest Windows machine and WebKitGTK. S4 AC-10 HEIC thumbnail timing. S5 AC-11 x264 and NVENC on reference hardware. S6 PG 18 CI job and PgListener loss test. S7 Vite + SvelteKit WASM with a 100 MiB file; Argon2id on a low-end phone and Safari. S8 live provider sign-in flows against each provider. S9 Android upload latency on Pixel, Samsung, Xiaomi. S10 GPU passthrough inside the no-network worker container. S11 external review of the vault construction.

## Decisions
- **D1** (resolved) iOS background upload: foreground + system-scheduled in v1.
- **D2** (resolved) iOS shows other providers only when Apple is enabled.
- **D3** (resolved) Ship ffmpeg with x264; project is GPL-compatible.
- **D4** (resolved) Linux token file fallback with a warning.

## Still open
- **D5** Packaging and signing choices (deferred to the Deploy step).
- **D6** Test hardware for Intel/AMD/Apple GPU workers (deferred; those stay Experimental only).

## Round 2 changes (Mode B, solution_draft02)

| Component Area | Candidate | Change | Status now | Rationale |
|---|---|---|---|---|
| Worker sandbox | Docker `internal` network + supervisor/decoder split over a pipe | Replaces Unix-socket access to Postgres | Selected + runtime gate (S10) | b_security F1: decoder RCE must not reach DB or blob mount |
| Auth providers | openidconnect generic OIDC + GitHub (v1.0), Apple (v1.1) | Facebook removed; Google through OIDC | Selected + runtime gate (S8) | b_ops F2/F9: staging; self-hosters run OIDC providers |
| Desktop settings UI | Slint + rfd | Replaces Tauri as primary; Tauri becomes fallback | Selected + runtime gate (mve_round2 #1; software renderer only; spike S3 on Windows/macOS/weak hardware; Tauri fallback triggers in the card) | Owner decision; AC-16 and licence compatibility |
| Blob encryption at rest | AEAD envelope, per-blob key, operator master key | New component (owner decision) | Selected + runtime gate (mve_round2 #2: XChaCha20 + aead-stream, 64 KiB chunks; measured 1.16-1.25 GB/s) | b_ops F6; breaks zero-copy; master-key loss unrecoverable |
| Mobile uploader | shared Rust uploader | New component | Selected + runtime gate (mve_round2 #3: UniFFI 0.32.2 + frb 2.13.0; Android device spike S9) | b_clients B12: WorkManager Worker has no Flutter engine |
| NVENC | ffmpeg + NVENC | Opt-in, off by default, v1.1 | Selected for v1.1 | b_clients B18/B19: gain only 1.4x with CPU filters |
| Reverse proxy | Caddy | New default in Compose | Selected + runtime gate (install test, AC-22) | b_ops: nginx 1 MB body default rejects 2 MiB blocks |
| Cursor | `epoch.seq` per account | Replaces plain counter | Selected + runtime gate (S6) | b_server F5 |
| ffmpeg build | native AAC, no libfdk_aac, no nonfree | New constraint | Selected | b_ops F5 |
| Licence | AGPL-3.0 + App Store exception | Replaces "GPL-compatible" wording | Selected (owner decision) | b_ops licence matrix |

New runtime gates: S12 browser 50 GiB upload and download; S13 reconciliation sweep after crash between blob write and commit; S14 AC-5 under 100 background uploads on XFS vs ext4.
