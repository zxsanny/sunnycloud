# Mode B assessment: operability, licences, scope and complexity (draft01)

Date: 2026-11-03. Reviewer stance: adversarial. Inputs: `00_problem/*.md`, `01_solution/solution_draft01.md`, `00_research/06_component_fit_matrix.md`, `00_ac_assessment.md`. Source numbers refer to the Sources section. "[limited source]" = not confirmed by a primary page.

Headline: draft01 is a technically careful design of about eight deliverables in six languages with no staging. The weakest parts are not in the protocol but around it: first-admin bootstrap, client/server compatibility policy, at-rest encryption (no component at all), backup/upgrade (explicitly unwritten), an identity model that does not match what self-hosters run, and D3, which is mis-stated (GPL ffmpeg does not force the project licence).

## Findings table

| # | Old component/decision | Weak point | Severity | Evidence | Recommended change | ACs affected |
|---|---|---|---|---|---|---|
| F1 | Social-only login + invite/allowlist + identity = (provider, id), never email | No way to create the first admin. A fresh install has an empty allowlist; operator does not know their own Google `sub` or GitHub numeric id in advance. Either nobody can log in, or "first login wins admin" on a public URL (takeover race). Immich avoids this with a local-password onboarding screen, which sunnycloud forbids | Critical | 22, 39 | Bootstrap by one-time setup token: operator sets `SETUP_TOKEN` (or the server prints one at first start); first sign-in that presents it becomes admin, then the token is voided. Invites match a verified email (or GitHub login) only at redemption, then bind to (provider, sub). Add a `sunnycloud admin` CLI subcommand (invite, list, revoke) so no admin UI is needed on day one | AC-22, AC-23, AC-19 |
| F2 | Whole scope in one release (API, workers, GPU, vault, 4 OAuth, web, Android, iOS, 3 desktop OSes, Tauri settings) | About 8 deliverables, 6 languages, 3 signing/packaging chains, ~11 spikes before any release. Immich (a funded team) needed ~3.7 years from first commit to a stable 2.0 and still calls HLS and workflows incomplete in 2026. No staging, no cut line in the draft | Critical | 21, 17, 39 | Adopt the staged roadmap below; make vault, iOS, macOS, non-NVIDIA GPU, Facebook and the Tauri window explicit post-v1 or gated items. Needs owner waiver of the affected ACs per quality-thresholds rule | all |
| F3 | D3: "ship ffmpeg+x264, so the project is GPL-compatible" | Non sequitur. ffmpeg is invoked as a subprocess; FSF FAQ treats pipes/sockets/command-line as communication between separate programs and says containers do not change that analysis. GPL attaches to the ffmpeg layer (source offer for ffmpeg+x264), not to the Rust code. The project licence can be chosen on its own merits (permissive, AGPL, GPL) | High | 14, 7, 9 | Reword D3: "GPL applies to the ffmpeg/x264 layer of the worker image; project licence is a separate decision D3b". Add a source-offer line to the image (Debian source packages pinned) | AC-11, AC-28; release |
| F4 | Licence choice (A)GPL for the mobile apps, silent on Apple | Apple's terms conflict with GPL; VLC was pulled in 2011 on a contributor's complaint. Precedents ship anyway: Nextcloud iOS = GPLv3 + explicit App Store exception file (copyright holders promise not to enforce the conflict); Signal-iOS AGPLv3; Ente AGPL-3.0, all on the App Store. The risk is third-party copyright holders who did not grant the exception | High | 1, 2, 3, 5, 15 | If clients are copyleft: sole copyright or CLA/DCO-with-exception from day one, plus a Nextcloud-style `COPYING.iOS`. Simpler: license mobile and desktop clients permissively (Apache-2.0) and the server AGPL-3.0 (network-separated works). Decide before the first external contribution is merged | AC-19; iOS release |
| F5 | AAC encoder unspecified; ffmpeg build flags unspecified | `libfdk_aac` is non-free and GPL-incompatible: an ffmpeg built with it plus `--enable-gpl` needs `--enable-nonfree` and cannot be redistributed. CUDA filters `scale_npp`/`--enable-cuda-nvcc` are also non-free [limited source] | High | 7, 8 | Pin: native `aac` encoder, `libx264`, NVENC via ffnvcodec, `scale_cuda` or CPU scaling; no `--enable-nonfree`. Prefer Debian-packaged ffmpeg (already GPL, no nonfree). Add a CI check `ffmpeg -version \| grep -v nonfree` | AC-11, AC-28 |
| F6 | "Non-vault data is encrypted at rest on the server" (problem, security_approach) | Appears in no AC and in no component of draft01. If implemented in-app it adds key management, key backup, worker key access in a no-network sandbox, scrub and resume interplay. If delegated to LUKS/ZFS it is an install-doc item. Currently nobody owns it | High | 39 | Decide explicitly: at-rest = operator-provided volume encryption (LUKS/ZFS native) for blobs and PG, documented in install and backup guides; add one AC ("install docs include a verified encryption-at-rest recipe"). App-layer encryption only if a named threat requires it | restriction "Encryption", AC-24, AC-25 |
| F7 | No API versioning or client/server compatibility policy | Three independently updated client families (store-reviewed mobile apps, desktop daemons, web) against operator-upgraded servers. Immich only gained "any 2.x app works with any 2.x server" at stable and had breaking changes on most releases before | High | 21 | `/api/v1` prefix, `X-Client-Version`, server-published `min_client_version`, one stated compatibility promise (e.g. server N supports clients N and N-1); sync-protocol and vault format carry version fields from the first commit | AC-5, AC-21; all clients |
| F8 | Backup/restore, upgrade, observability, quota, admin UI = "carried forward" | AC-22 (30 min install) and data safety depend on them. Immich's docs show what is needed: DB dump + `UPLOAD_LOCATION` copy, ordering (DB first, then files), restore drill, version-matched restores. Sunnycloud's immutable blobs make a safe ordering easy but only if GC has a grace period | High | 20, 25 | Write the procedure now (see Operator section). Add a CI "backup-restore drill" job (dump, wipe, restore, scrub, compare hashes) | AC-22, AC-24, AC-27 |
| F9 | Identity: Google, Apple, Facebook, GitHub only | Audience mismatch: the 2025 selfh.st survey (n=4,081) shows self-hosters run Authelia 390, Pocket ID 310, authentik 268, Pangolin 241, Keycloak 158 for auth; none of the four consumer providers is a self-hoster default. Facebook adds app review, privacy policy and data-deletion URL and an unusable "Live mode" step for a self-hosted install; Apple adds US$99/yr, Services ID, domain verification and key handling. Both break the 30-minute budget | High | 23, 30, 31, 6 | v1: one generic OIDC client (Google is just a preset of it) + GitHub (OAuth2, not OIDC). Apple only with the iOS release (guideline 4.8). Facebook: drop or v2. Never persist provider access/refresh tokens (also satisfies 5.1.1(v) and avoids Google's 7-day token issue) | AC-19, AC-22, AC-23 |
| F10 | Protocol client implemented per platform | Block protocol (BLAKE3 per block, blocklist, commit, resume) must exist in Rust (desktop), TypeScript (web), Kotlin (Android background), and Dart/Swift (iOS): at least 4 copies, each needing the AC-3/7/8 test suite | High | 39 | Mobile: one Dart implementation run from the background isolate (WorkManager/BGTask plugin) with only the MediaStore trigger native; Rust core only where a shared binary already exists (desktop, vault). Or one shared `sync-core` crate with bindings; pick one, justify in writing | AC-3, AC-7, AC-9 |
| F11 | iOS: D2 "other providers shown only if Apple enabled" | Makes the iOS app unusable on any server without Apple configured (operator pays US$99 and configures Services ID). Guideline 4.8 exemptions (own account system, enterprise/education accounts) do not obviously cover a social-only self-hosted app. Immich passes review because it has local passwords | High | 6, 40 | Before building iOS, ask App Review (pre-submission inquiry) whether an operator-configured OIDC IdP counts as "own account system" [limited source]; otherwise iOS ships with Apple required. Keep iOS out of v1 | AC-9, AC-19 |
| F12 | Reverse proxy "optional", buffering off, "raise body limits" | Operator-facing detail missing. Who terminates TLS; nginx default body limit is 1 MB (below the 2 MiB block) so default nginx / Nginx Proxy Manager answers 413 [limited source on NPM defaults]; proxy mix is NPM 842, Traefik 751, Caddy 598, Cloudflare Tunnel 563, nginx 528. Real client IP for AC-20 depends on trusted-proxy config: wrong setting rate-limits everyone as one IP or lets X-Forwarded-For spoof | Medium | 23, 24, 26 | Ship Caddy in the default compose (auto-TLS, no request buffering by default); document nginx (`client_max_body_size 0; proxy_request_buffering off`), Traefik, Cloudflare Tunnel. 2 MiB blocks stay below Cloudflare's 100 MB request cap, a design strength to keep. Config option `TRUSTED_PROXIES` mandatory-with-default-deny | AC-2, AC-20, AC-22 |
| F13 | Sandbox = `network_mode: none` + Unix-socket access to Postgres | Sharing a PG socket directory between two containers is a uid/permission and PG 18 path (`/var/lib/postgresql`, versioned PGDATA) papercut for operators | Medium | 25 | Use a Compose network with `internal: true` containing only Postgres and the worker (no egress); same isolation goal, no socket volume. Keep cgroup limits | AC-25, AC-22 |
| F14 | Transcode every video (H.264 1080p AAC) | Immich default policy is `required`: transcode only HDR, non-yuv420p or unsupported codecs, 720p, preset ultrafast; "disabled" and "all" are options. Transcoding everything doubles storage for already-playable files (Immich: thumbs+transcodes add ~10-20%) and loads workers for no gain. Streaming is out of scope, so the only purpose is playback compatibility | Medium | 18, 19 | Policy setting, default `required`-style: probe with ffprobe; H.264/AAC yuv420p under a bitrate cap is served as-is. Makes AC-11 matter for fewer files | AC-11, AC-28 |
| F15 | GPU: NVENC + VAAPI + QSV + VideoToolbox in AC-28 | Three of four unverifiable without hardware (D6); Immich still calls hardware transcoding experimental, supports Linux only, and warns of larger/lower-quality output; passthrough into a no-network container is spike S10. AC-11's 10-minute CPU target holds without a GPU | Medium | 17, 39 | v1: CPU only. v1.1: NVENC only (documented NVIDIA Container Toolkit recipe). VAAPI/QSV/VT: docs-only "community, untested" until hardware exists | AC-28, AC-11 |
| F16 | Vault in v1 (WASM + frb + Argon2id + recovery key + link share with key fragment) | Three bindings of one crypto core, Vite/WASM spike S7, Cargokit archived, mandatory external audit S11, a permanent on-disk format. Comparable products limit the feature: Nextcloud E2EE has no web access, no public links, no server versioning/search, folder-level user sharing only; Ente is E2EE-first and funded audits (Cure53, Symbolic, Fallible). Sunnycloud promises web access and link sharing, harder than Nextcloud's own E2EE | High (schedule) | 32, 5, 39 | Defer to v1.2 behind the audit gate; reserve only the `vault` folder flag and header version fields in v1. Do not ship an unaudited vault holding user data | AC-13, AC-14 |
| F17 | Desktop on 3 OSes + Tauri settings process + JSON-RPC socket | Per-OS cost is linear: watcher spike (S2 incl. Windows Defender), autostart, keychain, tray quirks (macOS main thread), signing/notarization, packaging (deb/rpm/AppImage/flatpak, MSI, dmg). Settings window cold start unmeasured (S3). D5 packaging is undecided yet gates release | Medium | 29, 27, 39 | Stage Linux, then Windows, then macOS. Ship daemon + tray + `config.toml`/CLI first; Tauri window as soon as S3 passes; Slint stays fallback. Windows signing: SignPath Foundation is free for OSI-licensed OSS (publisher shown as SignPath Foundation; needs OSI licence, no proprietary components, MFA) | AC-15..18, AC-26 |
| F18 | Observability absent | Operators cannot answer "is it stuck?": queue depth, disk free, scrub results, 5xx rate | Medium | 39 | `/healthz` (liveness), `/readyz` (DB + disk headroom), `/metrics` Prometheus text on a separate internal port, JSON logs to stdout. No OpenTelemetry/tracing stack in v1. Compose `healthcheck:` on every service | AC-22, AC-24 |
| F19 | Quotas and disk-full: "enforced before commit" with no design | Needs a per-user quota column, a global free-space reserve that refuses commits, and GC grace interplay; also the 50 GiB uploads (AC-4) can fill disk mid-upload | Medium | 39 | Quota = integer per user (default from env, admin override via CLI); pre-size staging file with `fallocate` so disk-full fails at start, not at commit; refuse when free < reserve | AC-4, AC-1 |
| F20 | Postgres 18 in Docker | Image changed in 18: volume at `/var/lib/postgresql`, PGDATA versioned (`/var/lib/postgresql/18/docker`); wrong mount path silently loses data on recreate. Major upgrades are dump/restore or pg_upgrade, not a pull | Medium | 25 | Compose uses the 18 layout; docs: `pg_dump` before every app upgrade; migrations forward-only, run at start under an advisory lock; never auto-pull majors (Watchtower warning in Immich docs) | AC-22 |
| F21 | HEIC thumbnails via libheif + libde265 | HEVC decode is patent-encumbered; software decoder licences grant no patent rights; enforcement against HEVC/AV1 continues (Dolby v. Snap, March 2026). Immich/Nextcloud ship the same stack, so industry practice, but risk sits with the distributor | Low | 11, 12, 16 | Install via Debian packages; add README liability note; do not bundle x265 (encoder, GPL) at all; operator may disable HEIC via config | AC-10 |
| F22 | OpenAPI + 4 provider mock + 11 spikes + per-OS matrix in CI | CI cost: macOS/Windows runners, Android/iOS builds, GPU tests (no GPU runners), PG 18 | Medium | 39 | CI tiers: per-PR (server, web, Linux desktop, Android build), nightly (Windows, macOS), manual (GPU, live providers, real devices) | all |
| F23 | Mobile apps published by whom? | Draft assumes store apps connecting to any server URL; needs a project-owned Apple Developer (US$99/yr) and Google Play account, privacy policy, App Privacy labels, in-app account deletion (5.1.1(v)). Not mentioned | Medium | 6 | Add an owner decision D7: publisher accounts and who bears the cost; document F-Droid/APK sideload for Android as the no-account path | AC-9, AC-27 |
| F24 | Slint fallback "usable because GPL" | Slint is GPL-3.0-only OR royalty-free OR commercial. GPL-3.0-only makes the desktop UI binary GPLv3 (compatible with AGPLv3 via section 13, not with GPLv2-only code). The royalty-free option requires attribution badge and forbids exposing Slint APIs | Low | 27, 28 | If desktop clients are permissive, choose Slint royalty-free (open-source projects owe no royalty) or stay on Tauri | AC-16 |
| F25 | Email/SMTP | Not required: no passwords, no reset flows, invites can be copy-paste links or allowlist entries | Low (positive) | 39 | Do not add SMTP in v1; record the decision | AC-23 |

## Complexity audit table

Count (draft01): deployables 8 (API, workers x2 lanes, Postgres, desktop daemon, Tauri settings, web, Flutter app, optional proxy); languages 6 (Rust, TypeScript/Svelte, Dart, Kotlin, Swift, SQL) + WASM; protocols/formats 7 (block sync, long-poll, JSON-RPC socket, OIDC, OAuth2, RFC 8628, vault format); auth integrations 4 + 1 custom grant; bindings 3 (WASM, frb, native).

| Item | Justified by AC | Cut / defer / keep | Reason |
|---|---|---|---|
| API (axum) + Postgres 18 + blob store | AC-1..8 | Keep | Core |
| Block protocol, 2 MiB blocks, 4 lanes, long-poll | AC-2..5 | Keep | Core; 2 MiB bodies also survive default proxies and Cloudflare's 100 MB cap |
| Postgres job queue (SKIP LOCKED) | AC-10, AC-11 | Keep | No Redis; scrub and transcode share it |
| Scrub job | AC-24 | Keep, as a queue job | No extra service |
| Thumbnail worker (libvips) | AC-10 | Keep | Core media feature |
| Transcode worker, CPU x264 | AC-11 | Keep, default-skip policy | Same image as thumbnail worker, lane flag |
| NVENC | AC-28 | Defer to v1.1 | AC-11 CPU target met without GPU; spikes S5, S10 |
| VAAPI / QSV / VideoToolbox | AC-28 (experimental) | Cut from AC, docs only | No hardware (D6), already "experimental only" |
| Worker sandbox | AC-25 | Keep, simplify | `internal: true` network instead of socket volume |
| Device-grant server (RFC 8628) | AC-19 desktop | Keep with first desktop | Needed because Apple has no device flow; small |
| OIDC client (generic) + Google preset | AC-19 | Keep v1 | One code path |
| GitHub OAuth2 | AC-19 | Keep v1 | Minutes to configure |
| Apple provider | AC-19, guideline 4.8 | Defer to iOS release | Cost, key handling, only needed for iOS |
| Facebook provider | AC-19 | Cut / v2 | Review burden; no store requires it; audience uses self-hosted IdPs |
| Sessions with refresh rotation + reuse detection | AC-21 | Keep | Needed for revocation in 5 s |
| Invite/allowlist + admin role | AC-23 | Keep, CLI-first | Plus setup token (F1) |
| Admin web UI | none explicit | Defer; v1 = CLI + 1 read-only page | Users, invites, queue depth, disk |
| Share links (+ password, expiry, revoke) | AC-12 | Keep | Core |
| Quotas | none explicit | Keep minimal (F19) | Disk safety |
| Account deletion | AC-27 | Keep | Store and GDPR requirement |
| Rate limiting (per code/account/link/IP) | AC-20 | Keep, in-DB counters | No Redis; trusted-proxy config |
| At-rest encryption | restriction, no AC | Decide (F6) | Prefer volume-level |
| Svelte web app (browse, upload, share) | problem | Keep | v1 |
| Web vault (WASM) | AC-13 | Defer v1.2 | Audit gate, S7 |
| Flutter app, Android backup (Kotlin trigger) | AC-9 | Keep v1 | The mobile-backup gap is the market hook |
| iOS app | AC-9 (best effort) | Defer v1.1 | Apple provider, review risk, best-effort background |
| iOS 26.1 upload extension | none | Cut (already deferred) | Fixed Info.plist destination |
| frb + Cargokit | vault only | Defer with vault | Archived build tooling |
| Desktop daemon: Linux | AC-15..18, 26 | Keep v1 | Dogfood platform |
| Desktop daemon: Windows | same | Keep v1 GA gate | Largest desktop share [limited source] |
| Desktop daemon: macOS | same | Defer v1.1 | Notarization, main-thread tray, TCC |
| Tauri settings window | AC-16, AC-17 | Keep, sequenced after daemon | S3 spike; CLI/config first |
| Slint fallback | AC-16 | Keep as contingency only | Do not build in parallel |
| Desktop packaging/signing x3 | distribution | Stage with OS | SignPath for Windows |
| OpenAPI | restriction | Keep | Generated from handlers |
| Prometheus `/metrics`, healthz | none | Add minimal | F18 |
| OpenTelemetry | none | Cut | Liability without a requirement |
| SMTP | none | Cut | F25 |
| Dedup/CDC, S3 backend, HTTP/3, HLS, version history | out of scope | Stay out | Already rejected |

## Proposed staged roadmap

Reasoning for order: ship what proves the differentiators (fast sync, simple install, camera backup) first; delay everything that needs hardware you do not own, an external audit, a paid store/relationship, or per-OS work multiplied by three. Each stage needs an owner waiver for the listed ACs.

| Stage | Contents | ACs delivered | ACs deferred |
|---|---|---|---|
| v1.0 | API, Postgres 18, blob store, block protocol, long-poll, scrub, quotas, shares, thumbnails (incl. HEIC), CPU transcode with `required`-style policy, generic OIDC (+Google preset) + GitHub, setup token + CLI admin, Svelte web, Android app with backup, Linux + Windows desktop daemon (tray, config file/CLI; Tauri window when S3 passes), Caddy compose, backup/restore drill, healthz/metrics, API versioning | 1-12, 15-24, 25, 26, 27, AC-9 (Android part) | AC-13, 14 (vault), AC-28, AC-9 iOS, macOS |
| v1.1 | macOS daemon, iOS app (foreground + BGTask) + Apple provider, NVENC (documented recipe), signing/notarization pipeline | AC-9 iOS, AC-28 (NVENC), AC-19 Apple | VAAPI/QSV/VT |
| v1.2 | Vault: Rust core, WASM, frb, link-sharing page, external crypto review (S11) before release | AC-13, AC-14 | none |
| v2 | VAAPI/QSV/VideoToolbox after hardware tests, iOS 26.1 extension (if device test allows custom server), Facebook if demanded, optional pre-sized HLS, multi-node/S3 backend, admin UI growth | extras | n/a |

Riskiest pieces for schedule (descending): (1) vault chain: WASM + frb + audit + format permanence; (2) iOS: App Review (4.8, arbitrary server URL, account deletion), Apple provider, background best effort, publisher account; (3) desktop x3: S2 watcher per OS incl. Windows Defender, signing, packaging, tray; (4) Tauri window cold start (S3); (5) GPU in sandbox container (S10) and no test hardware; (6) four live OAuth providers (Apple key and domain verification [limited source], Facebook review); (7) Android OEM background kill (S9). Pieces that are cheap: block protocol server, job queue, shares, scrub.

How comparables staged (for calibration):
- Immich: photos-first (server, web, mobile on both OSes), password login with optional OIDC, no E2EE, hardware transcoding still "experimental", breaking changes on most releases until stable 2.0 (Oct 2025), v3.0 July 2026 [21, 17].
- Nextcloud: sync and sharing first; E2EE added later with large feature losses (no web, no public links) [32]; admin pain concentrated in upgrades, DB growth, tuning [33].
- Syncthing: sync engine only, no accounts, MPL-2.0; no first-party mobile in the repo [36, limited source].
- Ente: E2EE-first product; AGPL-3.0; audited by three firms; server in the same monorepo; apps on App Store [5].
- Filen: AGPL-3.0 clients [37]; server licence unverified [limited source]. Seafile: optional per-library encryption [limited source].
- OpenCloud (oCIS fork, Apache-2.0, Go, no database): shows the market also moves to simpler storage; draws attention to the Postgres dependency being a choice, not a given [35].

## Operator experience (AC-22) design notes

AC-22 holds only with: one provider already registered (GitHub ~2 minutes; Google Cloud console longer, still within budget; Apple/Facebook out of budget), Caddy in the compose, and a bootstrap that needs no database poking.

Recommended compose layout (5 services, 1 image reused): `caddy` (TLS, 80/443), `api`, `postgres:18`, `worker-thumb`, `worker-transcode` (same image, `LANES` env; GPU stanza commented). Volumes: `blobs/`, `pgdata` mounted at `/var/lib/postgresql`. One `.env` with: `PUBLIC_URL`, provider client id/secret (secrets via Docker secrets or env file mode 0600), `SETUP_TOKEN`, `TRUSTED_PROXIES`, `BLOB_FREE_RESERVE`.

| Topic | Recommendation |
|---|---|
| TLS | Caddy terminates, proxies to `api:8080`; document external proxies with body-size and buffering lines (F12) |
| First-run | Setup token (F1); first admin created; token voided; OAuth redirect URIs printed by `sunnycloud doctor` for each enabled provider |
| OAuth app registration | `doctor` command prints exact redirect URIs and required console steps; Apple/Facebook pages marked "advanced" |
| Secrets | Env/Docker secrets only; Apple `.p8` as file secret; server generates the ES256 client secret (avoids Apple's 6-month JWT expiry trap) [31] |
| Upgrades | `docker compose pull && up -d` for app; migrations forward-only at start under advisory lock; mandatory `pg_dump` note; never auto-update PG majors; release notes list breaking changes (Immich recommends reading notes and warns about Watchtower) [20, 25] |
| Backup | Order: `pg_dump` first, then copy `blobs/` (restic/rsync/ZFS snapshot). Blobs are immutable and written before the DB commit, so DB-then-blobs yields only orphan blobs; GC must delete blobs only after a grace period longer than the backup window. Restore: load dump, restore blobs, run scrub, then start API. Back up the encryption/volume key separately (F6) |
| Restore drill | CI job runs the whole sequence on a test dataset each release |
| Monitoring | healthz/readyz/metrics (F18); alert on: queue age, disk free, scrub mismatch count, 5xx rate |
| Sizing for 500 users [limited source for exact numbers] | API + PG host: 4 vCPU, 8 GB RAM, PG on local SSD (Immich: DB 1-3 GB typical, never on network share, at least 2 GB RAM if limited); AC assessment estimates 10-15 GB DB at 25M rows; blobs: user data plus 10-20% for thumbs/transcodes (Immich figure) [19]; CPU worker: 4 physical cores per AC-11; measure in S1/S6 and publish |
| Disk/quota | Per-user quota, global reserve, `fallocate` staging (F19) |
| Admin UI | v1: CLI + one read-only page (users, invites, queue, disk, provider status) |
| Email | None (F25) |
| What admins complain about | Nextcloud: slowness from missing cron/cache/indices, DB growth, PHP/OS version gating upgrades [33]; Immich: breaking changes, hardware-transcode setup, iOS background backup stopping unless the app is opened [17, 34]. Design answer: fewer knobs, API compatibility promise, honest iOS docs |

## Licence matrix

Premise: choose project licence P in {Apache-2.0, AGPL-3.0, GPL-3.0}. Compatible = may be combined in one distributed work under P. Subprocess-only items (ffmpeg, x264) never combine with project code.

| Dependency class | Licence | With AGPL/GPL-3.0 | With Apache-2.0 project | Notes / source |
|---|---|---|---|---|
| axum, hyper-util, tokio, hyper | MIT | OK | OK | [28] |
| sqlx, reqwest, oauth2, argon2, chacha20poly1305, aead-stream, wasm-bindgen, tray-icon, keyring, tauri, tauri-plugin-dialog | MIT or Apache-2.0 | OK (Apache-2.0 is GPLv3-compatible, not GPLv2-only) | OK | [28] |
| openidconnect, jsonwebtoken, rusqlite, rfd, flutter_rust_bridge | MIT | OK | OK | rusqlite bundles SQLite (public domain) [28] |
| blake3 | CC0-1.0 or Apache-2.0 | OK | OK | [28] |
| notify | CC0-1.0 (9.0.0-rc.7) | OK | OK | The draft's "CC0?" is confirmed; debouncer-full MIT/Apache [28] |
| ksni | Unlicense | OK | OK | [28] |
| Slint | GPL-3.0-only OR royalty-free OR commercial | OK (GPL-3.0-only; AGPL via section 13) | Use royalty-free option (attribution) | [27, 28] |
| Svelte 5 / SvelteKit / Vite, Flutter, Dart | MIT / BSD-3 | OK | OK | [limited source], run an npm/pub licence scan in CI |
| Android WorkManager | Apache-2.0 | OK | OK | [limited source] |
| PostgreSQL | PostgreSQL licence | OK (separate service) | OK | official image [25] |
| Caddy | Apache-2.0 | separate service | OK | [limited source] |
| libvips | LGPL-2.1-or-later | OK, dynamic link / subprocess | OK | [13] |
| libheif | LGPL (sample apps MIT) | OK | OK | [11] |
| libde265 | LGPL | OK | OK | HEVC patents not licensed by the copyright licence [12, 16] |
| x265 | GPL-2.0 [11 lists it as GPL] | Do not ship (encoder only) | Do not ship | Not needed for decode |
| dav1d | BSD-2 [limited source] | OK | OK | AVIF |
| ffmpeg (default build) | LGPL-2.1+ | OK | OK | [7] |
| ffmpeg `--enable-gpl` | GPL as a whole | Separate program via exec/pipes | Separate program; no effect on project code | [7, 14] |
| x264 | GPL-2.0 (VideoLAN also sells a commercial licence) | Subprocess only | Subprocess only | [9] |
| libfdk_aac | non-free, GPL-incompatible | Must not be built in | Must not be built in | Use native `aac` [8] |
| ffmpeg native `aac` encoder | LGPL part, stable since 3.0, un-encumbered | OK | OK | [8] limited source |
| NVENC via ffnvcodec headers | MIT headers; driver libs injected from host by NVIDIA Container Toolkit, not redistributed | OK | OK | [limited source] |
| CUDA filters (libnpp, nvcc) | NVIDIA proprietary, non-free | Avoid | Avoid | [limited source] |
| OpenH264 (Cisco) | BSD source; Cisco binary patent coverage only if end user downloads it separately from Cisco with user control and attribution | Awkward in a no-network image | Awkward | [10]; rejected: baking it into the image voids the patent grant, quality/profile below x264 |
| Hardware encoders (NVENC, VideoToolbox) | vendor patent licences carried by hardware | OK | OK | Reduces H.264 exposure, not the build question |
| H.264 / HEVC patents | MPEG LA / Access Advance etc. | Distributor/operator risk | same | [7, 16] FFmpeg states it does not assess patents |
| Provider APIs (Google, Apple, Facebook, GitHub) | Provider terms, not software | n/a | n/a | Operator registers own apps |
| Apple App Store | Apple developer agreement and Standard EULA | GPL conflict, see D3 conclusion | OK | [1, 2, 6] |
| Google Play | Play distribution agreement | GPL apps ship (Nextcloud, Bitwarden) | OK | [4] limited source |
| Sign in with Apple | Platform service, no SDK required for the web/OIDC flow | n/a | n/a | Do not use Google Sign-In SDK on iOS; web flow keeps licence surface at zero |

D3 conclusion (needs revisiting, wording and consequence, not the engineering):
1. GPL does not reach the Rust code, the web app or the mobile apps merely because the worker image contains GPL ffmpeg/x264 (separate programs, [14]).
2. The iOS question exists only if the project chooses a copyleft licence for the Flutter app. Then use the Nextcloud pattern (GPL/AGPL + Apple exception, sole copyright or CLA) [1, 2], or license clients Apache-2.0 / MPL-2.0.
3. Options: (A) one licence for everything, AGPL-3.0 + `COPYING.iOS` exception, simplest file layout, matches Immich/Ente/Signal, forces Slint users to GPL-compatible, free SignPath signing OK; (B) server and worker AGPL-3.0, clients Apache-2.0: clean for the stores, two licence files, wide reuse of client code; (C) everything Apache-2.0, GPL confined to ffmpeg layer: most permissive, allows a closed hosted fork. Recommendation: (A) if the owner wants maximum simplicity and copyleft protection; (B) if app-store friction matters more. Whatever is chosen: decide before the first external contribution (copyright ownership) rather than "before the first public release".
4. Image distribution duty (any choice): publishing a worker image on a registry is distribution of ffmpeg+x264; include a source offer (pinned Debian source package versions) in the image label and README, or have operators `docker compose build` locally.

## Cost/benefit of distinct features

| Feature | Comparable products | Cost | Verdict |
|---|---|---|---|
| E2EE vault | Ente (all data), Filen (all data), Nextcloud (E2EE folders, crippled), Seafile (optional libraries), Cryptomator (vault-only); Immich, Syncthing none. Minority of self-hosted file/photo servers ship it, and those that do lose web/search/sharing | 3 bindings, audit, format permanence, user-facing limits (100 MiB cap, no camera backup into vault, single owner, key loss = data loss) | Defer v1.2; ship only after external review |
| Transcoding | Immich transcodes only when needed (default `required`, 720p), offers disabled | CPU load, storage, GPU setup | Keep, policy-driven, default skip for compatible files |
| GPU encode | Immich supports NVENC/QSV/RKMPP/VAAPI, experimental, Linux only | passthrough docs, quality tradeoff | NVENC v1.1 only |
| Four providers | Self-hosters prefer Authelia/Pocket ID/authentik/Keycloak [23] | each provider = distinct quirks, review, cost | Generic OIDC + GitHub v1; Apple with iOS; Facebook out |
| Desktop x3 | Syncthing/Nextcloud cover all three over years | linear per OS | Linux + Windows, then macOS |

## Market/competitor check (late 2026)

- Self-hosted file sync demand is real and concentrated: Nextcloud 1236 and Syncthing 1058 mentions in selfh.st 2025 "file storage and syncing"; Seafile 118, ownCloud 83; Immich ranks 3rd in favourite software (429) [23]. Immich is the proven answer for camera backup; v3.x released 2026 [21].
- Gap sunnycloud can credibly fill: block-sync performance (Nextcloud's assembly failures and slowness), Immich-class camera backup combined with arbitrary files, install in 30 minutes, one Rust binary plus Postgres (no PHP/Redis/ML stack). That gap is performance + simplicity, not E2EE, not four social logins.
- Competition: Immich (AGPL, adding sharing/workflows), Nextcloud 35 (Hub 26 Summer), OpenCloud (oCIS fork, Apache-2.0) [21, 33, 35]. Self-hosters' pain: Nextcloud tuning and upgrades [33], Immich breaking changes and iOS background backup [17, 34].
- Implication: market-fit risk is in identity (F9) and iOS (F11), not in sync.

## Sources

| # | URL | Tier | Date | Summary |
|---|---|---|---|---|
| 1 | https://github.com/nextcloud/ios | L2 | fetched 2026-11-03 | README: GPL-3.0 "with Apple app store exception" |
| 2 | https://raw.githubusercontent.com/nextcloud/ios/master/COPYING.iOS | L2 | 2026-11-03 | Copyright holders commit not to pursue violations solely caused by the App Store conflict, other GPL duties remain |
| 3 | https://github.com/signalapp/Signal-iOS | L2 | 2026-11-03 | AGPLv3, distributed on App Store |
| 4 | https://github.com/bitwarden/mobile | L2 | 2026-11-03 | GPL-3.0 mobile app on App Store; archived May 2025 (replaced by new native repos) |
| 5 | https://github.com/ente-io/ente | L2 | 2026-11-03 | AGPL-3.0 monorepo incl. server; E2EE; audits by Cure53, Symbolic Software, Fallible; on App Store, Play, F-Droid |
| 6 | https://developer.apple.com/app-store/review/guidelines/ | L1 | 2026-11-03 | 4.8 Login Services text and five exemptions; 5.1.1(v) account deletion and social credentials not stored off-device |
| 7 | https://ffmpeg.org/legal.html | L1 | 2026-11-03 | `--enable-gpl` makes whole FFmpeg GPL; LGPL checklist; patent disclaimer |
| 8 | FFmpeg wiki Encode/AAC (trac.ffmpeg.org/wiki/Encode/AAC) via search snippets | L2 [limited source] | 2026-11-03 | libfdk_aac non-free, GPL-incompatible; native aac encoder stable since 3.0 and license-unencumbered. Direct fetch blocked by Anubis |
| 9 | https://www.videolan.org/developers/x264.html | L1 | 2026-11-03 | x264 under GNU GPL (v2 text linked); commercial licence available |
| 10 | https://www.openh264.org/BINARY_LICENSE.txt | L1 | 2026-11-03 | Cisco binary patent coverage only when downloaded separately by end user, user-controllable, attribution required |
| 11 | https://github.com/strukturag/libheif | L2 | 2026-11-03 | LGPL; x265 GPL listed as encoder; libde265 default decoder |
| 12 | https://github.com/strukturag/libde265 | L2 | 2026-11-03 | LGPL; no patent statement |
| 13 | https://github.com/libvips/libvips | L2 | 2026-11-03 | LGPL-2.1-or-later; HEIC via libheif |
| 14 | https://www.gnu.org/licenses/gpl-faq.html | L1 | 2026-11-03 | Pipes/sockets/args are communication between separate programs; containers do not change work/aggregate analysis; page did not address App Store in the part read |
| 15 | https://9to5mac.com/2011/01/07/vlc-for-ios-removed-from-the-app-store/ (and related results) | L3 | 2011-01 | VLC removed after a contributor complained of GPL/App Store terms conflict |
| 16 | https://convergedigest.com/licensing-terms-announced-for-high/ and related search results | L3 [limited source] | 2014-15, dated | HEVC pool terms; software-layer carve-out statement; Dolby v. Snap mention March 2026 |
| 17 | https://docs.immich.app/features/hardware-transcoding | L1 | 2026-11-03 | NVENC/QSV/RKMPP/VAAPI, Linux only, experimental, larger/lower quality output, compose `extends` setup |
| 18 | https://docs.immich.app/administration/system-settings | L1 | 2026-11-03 | Transcode policy options and defaults (required, h264, aac, 720p, ultrafast) |
| 19 | https://docs.immich.app/install/requirements | L1 | 2026-11-03 | 6-8 GB RAM, 2-4 cores, DB 1-3 GB local SSD, thumbs+transcodes +10-20% |
| 20 | https://docs.immich.app/administration/backup-and-restore | L1 | 2026-11-03 | Auto DB dumps, back up DB first then files, restore paths, version-matched restores |
| 21 | https://immich.app/blog/stable-release ; https://immich.app/blog/2026-july-recap ; release posts via search | L1/L2 | 2025-10 to 2026-10 | 1,337 days since first commit to stable; 2.x compatibility promise; v3.0.0 July 2026; HLS/workflows incomplete |
| 22 | https://docs.immich.app/administration/oauth (search snippet) | L1 | 2026-11-03 | OAuth is configured after install; auto-register on by default; role from claim; mobile redirect URI |
| 23 | https://raw.githubusercontent.com/selfhst/cdn/main/assets/surveys/annual/2025-results.json | L2 | 2025 survey | n=4,081: proxies, file sync tools, auth tools, favourites (figures quoted above) |
| 24 | https://developers.cloudflare.com/workers/platform/limits/ | L1 | 2026-11-03 | Request body cap 100 MB Free/Pro, 200 MB Business |
| 25 | https://hub.docker.com/_/postgres | L1 | 2026-11-03 | PG 18 image: volume `/var/lib/postgresql`, versioned PGDATA |
| 26 | https://caddyserver.com/docs/caddyfile/directives/reverse_proxy | L1 | 2026-11-03 | Request/response buffering only when configured; no periodic flush by default |
| 27 | https://slint.dev/terms-and-conditions | L1 | 2026-11-03 | GPLv3 / royalty-free (attribution, no embedded) / commercial; open-source projects owe no royalty |
| 28 | https://crates.io/api/v1/crates/<name> (queried for 26 crates) | L1 | 2026-11-03 | Licence fields quoted in the matrix; notify 8.2.1 and 9.0.0-rc.7 both exist; rfd 0.17.2 exists (0.16.1 is a later backport) |
| 29 | https://signpath.org/ and search results | L3 | 2026 | Free signing for OSS, publisher is SignPath Foundation, OSI licence and MFA required |
| 30 | Facebook Login guides (n8n docs, Kreezalid, Logto) via search | L3 [limited source] | 2026 | Live mode needs privacy policy URL (data deletion URL per some); email/public_profile need no review; business verification not confirmed |
| 31 | Apple SIWA guides (Pomerium, better-auth) via search | L3 | 2026 | Client-secret JWT max 6 months; .p8 downloadable once; domain/return URL registration |
| 32 | Nextcloud E2EE architecture notes / RFC via search | L3 | dated | E2EE files: no web access/preview, no public links, no online office, folder-level user sharing only |
| 33 | Nextcloud snap/server GitHub issues, forum threads, wz-it Nextcloud 35 post | L4 | 2020-2026 | DB growth/upgrade failures, tuning (cron, cache, indices), PHP/OS gating of upgrades |
| 34 | Lemmy/forum threads on Immich iOS background backup | L4 | 2025-26 | Background upload stalls unless app opened; timeouts need restart |
| 35 | https://heise.de/-10254438 ; https://opencloud.eu | L3 | 2025 | OpenCloud: oCIS fork by Heinlein group, Apache-2.0, Go, no DB |
| 36 | https://github.com/syncthing/syncthing | L2 | 2026-11-03 | MPL-2.0; no mobile mention on the page |
| 37 | https://github.com/FilenCloudDienste/filen-desktop | L2 | 2026-11-03 | AGPL-3.0 client; server licence not stated |
| 38 | https://github.com/immich-app/immich | L2 | 2026-11-03 | AGPL-3.0; 3-2-1 backup warning |
| 39 | Project files (draft01, fit matrix, AC assessment, problem/restrictions/AC) | internal | 2026-11-03 | Basis for count, gaps, absences |
| 40 | https://gigazine.net/gsc_news/en/20240129-apple-sign-in-with-apple-remove | L3 | 2024-01 | Apple dropped the Sign in with Apple mandate but still requires an equivalent privacy-preserving login option |

## Items confirmed OK

- Block protocol with 2 MiB blocks: request bodies fit default Cloudflare and most proxies' limits except nginx's 1 MB default (documented, F12); long-poll 30 s fits proxy timeouts.
- Rust crate licences all permissive (MIT/Apache-2.0/CC0/Unlicense); notify is CC0-1.0; none block AGPL/GPL-3.0 or Apache-2.0 project licensing [28].
- Server-generated Apple ES256 client secret: avoids the six-month manual rotation trap [31].
- Postgres job queue, no Redis: matches Immich's Postgres-centred operation.
- libvips/libheif/libde265 are LGPL: fine as dynamic/subprocess use under any project licence.
- Native AAC encoder choice is available and unencumbered (F5 asks to pin it).
- Social-only login needs no SMTP (F25).
- ffmpeg as subprocess in an image: GPL scope stays on the ffmpeg layer (F3).
- Rejecting HTTP/3, CDC, S3 backend, iOS 26.1 extension for v1: consistent with simplicity rule.

## Open questions

1. Owner: accept staged roadmap and AC waivers (vault AC-13/14, NVENC AC-28, macOS, iOS, providers)? Which desktop OS is v1 minimum: Linux only, or Linux + Windows?
2. Project licence (D3b): A, B or C above; who holds copyright; CLA or DCO?
3. Is a generic OIDC provider acceptable as a scope addition even though problem.md lists four named providers?
4. At-rest encryption: volume-level (operator) or app-level?
5. Does Apple App Review accept an operator-configured OIDC IdP under 4.8's "own account system" exemption? Needs a pre-submission inquiry [limited source].
6. With a single store-published iOS app and per-operator servers, can Sign in with Apple use native ID-token validation (audience = the app's bundle id) instead of each operator's Services ID? [limited source]
7. Does Sign in with Apple's web flow still require a domain-association file served from the operator's host? Search results did not confirm [limited source].
8. Who publishes the mobile apps (Apple Developer, Google Play accounts, cost, privacy policy URL)? (D7)
9. Mobile protocol client: Dart-only background isolate, or shared Rust core with bindings (F10)?
10. Exact Debian ffmpeg build flags for NVENC and absence of nonfree: verify in the worker Dockerfile CI check (FFmpeg HWAccelIntro page could not be fetched) [limited source].
11. Security-audit cost for the vault: no price source gathered; obtain quotes before committing v1.2 date [limited source].
12. Cloudflare Tunnel use for large-file hosting on Free plan: check Cloudflare terms of service before documenting it as supported [limited source].
