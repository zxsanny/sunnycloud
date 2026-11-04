# Solution Draft 02

## Assessment Findings

Source reports: `_docs/00_research/raw/b_server_storage.md`, `b_security.md`, `b_clients_media.md`, `b_ops_licence_scope.md`. No Critical finding reversed a Selected storage, database, protocol or server-stack decision; most gaps were missing specification.

| Old Component Solution | Weak Point (functional/security/performance) | New Solution |
|------------------------|----------------------------------------------|-------------|
| Worker sandbox: no network, Unix-socket access to Postgres (Critical, security) | A decoder exploit (libheif, libvips and ffmpeg all had 2026 CVEs) reaches the database and blob mount | Supervisor/decoder split: the supervisor holds database credentials; the decoder child has no credentials, no blob mount, no network (Docker `internal` network); protocol and format whitelists |
| Social-only login with invite/allowlist (Critical, operability/security) | No way to create the first admin; whoever reaches a fresh install first could own it | One-time setup token plus `sunnycloud admin`; single-use invite tokens; email never used for linking unless the provider verifies it |
| Whole scope in one release (Critical, schedule) | About eight deliverables in six languages with no staging | v1.0 / v1.1 / v1.2 staging; Facebook cut; generic OIDC added |
| No delete or sync-root safety net (Critical, data loss) | A missing or unmounted root propagates mass deletes with no recovery | Root sentinel, mass-delete hold with confirmation, 7-day operator-only blob retention |
| Overwrite and delete unlink blobs immediately (High, restore) | A restored older database points at blobs already removed | GC queue written in the same transaction, retention at least the backup interval (default 7 days) |
| Block receipt undefined (High, durability) | Ack before flush plus O(1) finish can publish zeroed blocks | Block acknowledged only after batched fdatasync; received-bitmap in the upload session |
| Commit / PUT / commit per file (High, performance) | About 150 s for the 10,000-file set at 20 ms (reviewer estimate) | Single-request upload up to 2 MiB; batch commit endpoint |
| Quota checked at commit (High, DoS) | 100 sessions all pass the check and fill the disk | Reserve bytes at session start; per-account session cap; free-space floor |
| Per-account counter cursor (High, restore) | Restoring an older database rewinds the counter; devices skip changes | Cursor `epoch.seq`; unknown epoch or seq below purge horizon returns `resync_required`; tombstones 90 days |
| Device grant as designed (High, security) | Device-code phishing is mass-exploited in 2025-26; per-code attempt counting is not possible | Approval page shows requester vs approver IP/location; 10 min TTL; sync-only tokens; limits per approving session, account and IP |
| Vault AAD and header unspecified (High, security) | Wrapped key and names not bound to vault and file; KDF downgrade | Normative AAD binding, authenticated bounded KDF header, key-commitment value; external review before release; vault moved to v1.2 |
| User content served without rules (High, security) | XSS through uploaded HTML or SVG | Inline only for allowlisted image/video types; everything else attachment + nosniff + sandbox CSP |
| Desktop trusts server-supplied names (High, security) | Path traversal, reserved names, symlink escapes | Directory-handle writes, no symlink following, hostile-name corpus per OS |
| Desktop first sync, editors, placeholders, locks (High, correctness) | No merge rule; save-in-place breaks file IDs and share links; cloud-evicted files hydrate; locked files | Conflict copies on first merge; file-ID tracking across rename-over; skip evicted files; lock retry |
| Web 50 GiB upload/download (High) | No browser design; Safari/Firefox lack streaming save | Worker-based sliced upload with re-select and lastModified check; AC-7 stated per client |
| Mobile uploader: WorkManager Worker has no Flutter engine (High) | frb not callable there; protocol written three times | One Rust uploader shared by all entry points (UniFFI or JNI; MVE pending) |
| AC-11 reference clip (High, performance) | 1080p SDR is not representative; 4K HLG to 1080p SDR with tone mapping runs at 0.63x real time on 4 cores | Three reference clips, `transcode_policy: required`, derived storage cap |
| NVENC default-on (Medium) | CPU 1080p SDR already runs at 3.3x real time; NVENC with CPU filters gains only 1.4x and adds driver fragility | NVENC opt-in, off by default, v1.1 |
| Settings UI Tauri first (Medium) | WebView2 and WebKitGTK/NVIDIA/Wayland risk for AC-16 | Slint first, Tauri fallback |
| "Encryption at rest" undefined (High) | No component or AC | Application-level envelope encryption of blobs (owner decision); operator master key |
| No API versioning, backup, upgrade, reverse proxy (High, operability) | Clients and operators break on upgrade | `/api/v1`, client minimum version, tested backup/restore/upgrade, Caddy default |
| ffmpeg build flags unspecified (Medium, licence) | `libfdk_aac` and nonfree filters are not redistributable with GPL | Native AAC encoder, no nonfree build |
| Dependency floors (Medium, security) | tokio 1.54.0 yanked (use-after-free); h2, bytes, rustls advisories | Floors in restrictions; `cargo audit`/`deny` blocking |
| Licence D3 wording (Medium) | GPL applies to the worker image layer, not necessarily to the Rust code or apps | AGPL-3.0 with App Store exception file (owner decision) |
| ext4 fsync under mixed load (Medium, performance) | Small-file fsync can wait behind big writers; AC-5 at risk | XFS documented default; AC-5 tested under 100 background uploads |

## Product Solution Description

sunnycloud is a self-hosted file, photo and video sync service installed by an operator with Docker Compose. A Rust API server keeps the folder tree and metadata in PostgreSQL 18 and file contents as immutable, server-encrypted blobs on local disk. Sandboxed Rust workers on the operator's machines make thumbnails and transcode video. Every client uses one block-based HTTPS protocol: a Svelte 5 web app, a Flutter app for Android (iOS in v1.1) with a shared Rust uploader, and a native Rust desktop daemon with a tray and a small Slint settings window. An opt-in end-to-end encrypted vault follows in v1.2.

```
 Web (Svelte 5)      Android app (Flutter + Kotlin WorkManager)      Desktop daemon + tray (+ Slint settings)
       \                 |  shared Rust uploader                          /
        +------- HTTPS / HTTP-2, /api/v1, blocklist protocol, long-poll cursor -------+
                                   |
                          Caddy (TLS, no buffering)
                                   |
                       API server (axum/hyper, Rust)
             sessions, OAuth/OIDC, device grant, shares, quotas, GC queue
                  |                                   |
          PostgreSQL 18                     blob store (XFS/ext4), encrypted blobs
   tree, blocks, epoch.seq cursor, jobs      blobs/<2hex>/<2hex>/<id>
                  |
        worker supervisor (DB creds)  --pipe-->  decoder child (no creds, no mount, no network)
                                                  vipsthumbnail | ffmpeg (x264 CPU; NVENC opt-in v1.1)
```

## Architecture

All component areas were checked against the Project Constraint Matrix. Sub-matrices and per-mode API evidence: `_docs/00_research/06_component_fit_matrix.md` and `_docs/00_research/02_fact_cards/mve_*.md`. "Selected + gate" means the API is proven and a named spike must still pass. The three round-2 candidates (Slint, blob encryption, shared mobile uploader) are verified in `mve_round2.md`. Workers receive plaintext from an internal API endpoint (decrypted server-side), so neither the supervisor nor the decoder holds the master key or a blob mount; ffmpeg reads a decrypted scratch file because it cannot read a trailing-moov MP4 from a pipe.

### Component: Storage, metadata and encryption at rest

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| ID-named blobs + tree in Postgres | XFS (default) or ext4, PostgreSQL 18, sqlx 0.9 | Write to the final path under a `blobs` row in state `uploading`; flip state at commit; blocklist stored as one bytea; name uniqueness through a `name_key` column; per-account `epoch.seq` counter updated as the last statement before COMMIT | O(1) moves, one source of truth, reconciliation sweep for orphans | Operators cannot browse by name; no data checksums on the filesystem | PG 18 CI job; minor-update procedure | Row scoping by account plus a generated IDOR matrix (RLS is only defence in depth) | Hot-row contention per account to be measured | MVE: mve_server_media #2; docs: raw/storage_db, raw/b_server_storage | Selected + gate (S6) |
| Application-level blob encryption | chacha20poly1305 =0.11.0 (XChaCha20) + aead-stream =0.6.0 `StreamBE32` | Per-blob random data key, wrapped under the operator master key with XChaCha20 and a random nonce; 64 KiB chunks so Range reads stay cheap; AAD binds blob id, chunk index, final flag; AES-GCM not pinned (0.12 GB/s without AES hardware) | Stolen disk yields no plaintext; key rotation by rewrap | Forecloses sendfile zero-copy; master-key loss is unrecoverable; workers receive decrypted streams | Master key as file, env or Docker secret, backed up separately | Key never stored with data | Must keep AC-2 at 112 MB/s with decrypt | MVE: mve_round2 #2 (measured 1.16-1.25 GB/s single-core decrypt vs 0.112 GB/s needed; 4 KiB range read 57 µs) | Selected + gate (S1) |
| GC and retention | `blob_gc` queue written in the same transaction as the change | Retention default 7 days; account deletion immediate | Restore consistency | Disk held for the retention period | Operator CLI `restore <path> <time>` | | | raw/b_server_storage F1 | Selected |
| Path-mirror, chunked CAS, S3 backend, SQLite | | | | | | | | draft 01 | Rejected |

Integrity: BLAKE3 per block and per file in flight; scrub re-verifies stored blobs under throttle (AC-24); ZFS/btrfs mirror and ECC RAM recommended.

### Component: Sync protocol and server

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| Blocklist protocol over HTTPS | axum 0.8.9, hyper 1.12, hyper-util, tokio 1.54.1+, h2 0.4.16+, BLAKE3 | 2 MiB blocks, 4 lanes; block acked after batched fdatasync; received-bitmap in upload session; quota reserved at session start; single-request upload for files up to 2 MiB; `POST /api/v1/commits:batch`; base revision sent with each change; long-poll `GET /api/v1/changes?cursor=epoch.seq&wait=30` | Resume loses at most 8 MiB; small files cost one request | Hand-built accept loop with large H2 windows; per-route body limits; no 1xx responses | Caddy with buffering off; body limit above 2 MiB | Stalled-connection limits (AC-59); CSRF origin checks | S1 (AC-2), AC-5 under 100 uploads | MVE: mve_server_media #1, #3 | Selected + gate (S1) |
| HTTP/3, tus, S3-multipart | | | | | | | | draft 01 | Rejected |

### Component: Media pipeline

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| Thumbnails | `vipsthumbnail` CLI (libvips 8.18.7), libheif 1.22.0+ | 320 px WebP + 1440 px preview; HEIC profile separate; process pool question measured in S4 | Fast, isolated | HEIC has no shrink-on-load; per-file spawn cost at 10,000 files | Reserved thumbnail lane | `VIPS_BLOCK_UNTRUSTED=1`, decoder child | AC-10 incl. bulk import | MVE: mve_server_media #5 | Selected + gate (S4) |
| Video, CPU | ffmpeg + libx264, native AAC (no libfdk_aac, no nonfree) | H.264 High 1080p AAC MP4 faststart, veryfast/fast, CRF 23; transcode only if not already compliant; HDR tone mapping to SDR | Works everywhere | 4K HLG to 1080p SDR at 0.63x real time on 4 cores | Progress/timeout watchdog | Protocol and format whitelists | AC-11 three reference clips | MVE: mve_server_media #6 | Selected + gate (S5) |
| Video, NVIDIA | ffmpeg + NVENC | Opt-in, off by default, probe by test encode, CPU fallback | Faster on 1080p | CPU filters limit gain; container fragility; `RLIMIT_AS` < 4 GiB breaks it | NVIDIA container toolkit | | 1.4x on 1080p with CPU filters | MVE: mve_server_media #6 | Selected for v1.1 (gate S5, S10) |
| Video, Intel/AMD/Apple | VAAPI, QSV, VideoToolbox | | | Unverified | | | | | Experimental only (docs only) |
| Queue and sandbox | Postgres `jobs` (SKIP LOCKED); Docker `internal` network; supervisor/decoder split over a pipe | Decoder has no DB credentials, no blob mount, no network; CPU/memory/time limits; supervisor fetches plaintext from an internal API endpoint and writes results back through it (no blob mount, no master key) | Contains decoder RCE | Extra process per job | | | | MVE: mve_server_media #4, #7; raw/b_security F1 | Selected + gate (S10) |

### Component: Identity and sessions

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| Built-in OAuth/OIDC client | openidconnect 4.1.0, oauth2 5.0.0, jsonwebtoken | Generic OIDC (covers Google, Authelia, authentik, Keycloak, Pocket ID) and GitHub in v1.0; Apple in v1.1 (hand-written ES256 client secret, no scopes, no PKCE); identity (provider, id); Facebook removed | Matches what self-hosters already run | Live flows unproven | Operator registers apps | state/nonce/PKCE tests (AC-56); no email linking (AC-55) | | MVE: mve_crypto_auth_mobile #5 | Selected + gate (S8) |
| Bootstrap | One-time `SETUP_TOKEN` + `sunnycloud admin`; single-use invite tokens | | Closes first-admin hole | | | AC-23 | | raw/b_ops F1, b_security F4/F5 | Selected |
| Sessions and device grant | Opaque tokens SHA-256 hashed; refresh rotation with 30 s reuse grace; hand-written RFC 8628 | 10 min user code; approval page shows IP/location; sync-only device tokens | Revocation within 5 s | Hand-written | | AC-57, AC-58 | | MVE: mve_crypto_auth_mobile #6; raw/b_security F2/F3/F8 | Selected |

### Component: Vault (v1.2)

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| Shared Rust crypto core | chacha20poly1305 0.11.0 + aead-stream 0.6.0, argon2 0.6.0, wasm-bindgen, flutter_rust_bridge 2.13.0 | XChaCha20-Poly1305 STREAM (BE32, 19-byte prefix); AAD = vault id, file id, parent; authenticated header with KDF bounds; HKDF key-commitment value; recovery key wraps the vault key | One implementation everywhere | `aead-stream` has no stated audit | External review before release; Vite+SvelteKit 100 MiB spike | AC-60, AC-61, AC-62 | Argon2id about 128 ms native | MVE: mve_crypto_auth_mobile #1-#4 | Selected + gate (S7, S11) |

### Component: Desktop client

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| Daemon | notify 8.2.1 + debouncer-full 0.7.0, rusqlite 0.41.1, reqwest 0.13.5 | Hints only; own settle (2 s), echo suppression by file id/size/mtime/hash, rename-over tracking, safety rescans; mass-delete guard and root sentinel; skip evicted placeholders; lock retry; first-sync conflict copies | Cross-OS | Windows overflow silent in 8.2.1; Defender stalls | Per-OS spikes | Directory-handle writes, no symlink following (AC-37) | AC-15 | MVE: mve_desktop #1, #7, #8 | Selected + gate (S2) |
| Tray | tray-icon 0.26.1 (ksni feature) | Inside daemon; daemon works without tray | One crate for three OSes | macOS needs main thread | | | | MVE: mve_desktop #2 | Selected |
| Settings window | Slint 1.18.1 with the software renderer only (GPL-3.0-only route) + rfd 0.17.2 for native dialogs; JSON-RPC over a 0600 socket or ACL'd named pipe | Separate on-demand process; software renderer measured at 47-61 ms to first frame and about 17 MB RSS on Linux Wayland+NVIDIA, versus 157-424 ms and 121-136 MB with OpenGL | Native, small | Licence GPL-3.0 or royalty-free terms; toolkit unverified here | Cold-start spike S3 on weak Windows and Wayland+NVIDIA | IPC ACL (AC-48) | AC-16 | MVE: mve_round2 #1; fallback evidence: mve_desktop #3 | Selected + gate (S3: Windows, macOS, weakest hardware; fallback to Tauri on the five triggers in the card) |
| Autostart, token, updates | systemd user unit, Task Scheduler logon task, launchd; OS keychain with Linux user-only file fallback; signed updater, source fixed at build time | | | Signing decisions at Deploy | | AC-49 | | MVE: mve_desktop #5, #6, #9 | Selected |

### Component: Web and mobile clients

| Solution | Tools | Pinned Mode/Config | Advantages | Limitations | Requirements | Security | Performance | API Capability Evidence | Fit |
|----------|-------|--------------------|-----------|-------------|-------------|----------|-----------|-------------------------|-----|
| Web | Svelte 5, Web Worker sliced uploads | Resume after tab close by re-selecting the file and checking lastModified; downloads use the streaming save API where it exists | | Safari/Firefox download verification weaker | | CSP, inline types allowlist (AC-47) | AC-50, AC-51 | raw/b_clients_media B8/B9 | Selected + gate |
| Android | Flutter UI, Kotlin WorkManager + MediaStore trigger, user-initiated data-transfer jobs, shared Rust uploader | Checkpoint DB, MediaStore-generation catch-up | Play-compliant | Xiaomi/Huawei kill background work | Device matrix S9 | | AC-9, AC-52 | MVE: mve_crypto_auth_mobile #7; uploader: mve_round2 #3 (UniFFI 0.32.2 + flutter_rust_bridge 2.13.0 over one core crate, one library per platform) | Selected + gate (S9: NDK r28+ 16 KB alignment, JNA 5.17+, 150 MB RSS, forced-kill test) |
| iOS (v1.1) | Flutter, foreground catch-up, system-scheduled work | Best effort | Any server address | Slower | Sign in with Apple rule | | | MVE: mve_crypto_auth_mobile #8, #9 | Selected for v1.1 |

## Staged roadmap

| Release | Scope |
|---------|-------|
| v1.0 | Server, workers (CPU), web, Android, Linux and Windows desktop, GitHub + generic OIDC, encryption at rest, deletion safety, backup/restore/upgrade, Caddy |
| v1.1 | iOS, macOS desktop, Sign in with Apple, NVIDIA opt-in, Live Photo pairing |
| v1.2 | E2EE vault after external review |
| Later | Intel/AMD/Apple GPU, content-origin hostname, push notifications |

## Testing Strategy

### Integration / Functional Tests
- Upload 5 GiB, kill the connection at 50%, resume: final hash equal, at most 10 MiB re-sent (AC-3, AC-4, AC-7); `kill -9` of the server during 100 uploads leaves no zeroed blocks (AC-38).
- Two devices: edit on A appears on B within 5 s also under 100 background uploads (AC-5); stale offline edit produces a conflict copy; edit beats delete (AC-8).
- Restore an older database: devices get `resync_required` and converge (AC-41); blob retention and operator restore (AC-30); backup/restore/upgrade test in CI (AC-65).
- Desktop: mass-delete hold, first-sync merge, evicted placeholders, locked files, editor save-in-place, suspend/wake, hostile names (AC-29, AC-31 to AC-35, AC-37).
- Bootstrap and auth: setup token works once, no email linking, 10 negative OAuth callbacks, device-approval controls, refresh race, CSRF, two-account IDOR matrix (AC-23, AC-55 to AC-59).
- Content serving: html/svg/xml/js/pdf/polyglot always attachment; share tokens hashed and constant-looking (AC-12, AC-47).
- Decoder red-team corpus ends in job failure without exposure (AC-25).
- Vault (v1.2): swap, header fuzz, CSP and heap-scan tests (AC-13, AC-60 to AC-62).

### Non-Functional Tests
- S1 throughput at 1 Gbps with encryption at rest on and proxy buffering off (AC-2); listing 10,000 entries (AC-6); 10,000 small files in 60 s (AC-40).
- S2 100k-file watcher per OS; S3 Slint cold start on a weak Windows laptop and Wayland+NVIDIA (AC-16).
- S4 thumbnail timing and bulk import; S5 three transcode reference clips on CPU, NVENC in v1.1 (AC-10, AC-11, AC-28, AC-44, AC-45).
- S8 live provider flows; S9 Android device matrix; S10 decoder isolation with GPU passthrough; S11 external vault review; S12 browser 50 GiB upload and download (AC-50, AC-51).
- Supply chain: `cargo audit/deny` gate, image signing, weekly worker rebuild (AC-63).

## References
Round-1 reports and fact cards: `_docs/00_research/raw/{storage_db,sync_protocol,media_pipeline,security_clients}.md`, `_docs/00_research/02_fact_cards/mve_*.md`. Round-2 reviews: `_docs/00_research/raw/b_server_storage.md`, `b_security.md`, `b_clients_media.md`, `b_ops_licence_scope.md`. Fit matrix: `_docs/00_research/06_component_fit_matrix.md`.

## Related Artifacts
- Tech stack evaluation: not produced (stack choices are in the component tables).
- Security analysis: the threat model and 37 findings are in `_docs/00_research/raw/b_security.md`; Phase 4 write-up not produced.
