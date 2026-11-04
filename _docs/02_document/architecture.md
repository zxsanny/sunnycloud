# sunnycloud — Architecture

## Architecture Vision

sunnycloud is one self-hosted installation per operator. Users sync files, photos and video through a web app, an Android app (iOS later) and a desktop daemon. The server is a single Rust API plus separate Rust worker processes sharing one PostgreSQL 18 database and one encrypted blob volume. It is a modular monolith plus workers, not microservices.

**Components and ownership**
- **API server** — HTTP API under `/api/v1`; owns sessions, sign-in, the file tree, uploads, sharing, quotas, the change feed, garbage collection and blob encryption.
- **Workers** — a supervisor that takes jobs from the database queue, and a decoder child that makes thumbnails and transcodes video.
- **Web app** — Svelte 5; browsing, sliced uploads, sharing, admin. The vault page arrives in v1.2.
- **Android app (iOS later)** — Flutter UI with a native background uploader sharing one Rust core.
- **Desktop daemon and settings window** — a Rust daemon with a tray and a Slint settings window.
- **Shared Rust core** — the sync-protocol client, hashing, and the vault crypto (v1.2).
- **Operator tools** — `sunnycloud admin` command line, scrub, restore and doctor commands.
- **Caddy** — TLS and request routing.

**Data flows**
- **Upload** — a client sends the block list; the server names the missing blocks; the client sends them over up to 4 lanes; a commit publishes the file.
- **Sync** — devices long-poll the per-account change feed (`epoch.seq` cursor) and download what changed.
- **Media** — a commit queues jobs; the supervisor fetches decrypted input from an internal API endpoint; the decoder processes it with no credentials or network; results return through the API.
- **Sign-in** — the browser authenticates with the provider; the API issues its own session; desktop daemons get theirs through the server's own device-code grant.

**Principles**
- The database is the source of truth for the tree; blobs are opaque and immutable.
- The later commit wins, a stale edit becomes a conflict copy, and an edit beats a delete.
- The clients guard against mass deletes, and the server keeps replaced or deleted blobs for 7 days for operator restore only.
- The settings window uses native OS dialogs; no custom file browser.
- One upload implementation per client family.
- Every duration is configuration, so tests use short values.
- Decoders are untrusted and never hold the master key or database credentials.

**Confirmed decisions on open questions**
- Monorepo with a Rust workspace (server, worker, daemon, settings, shared core); the Svelte app and the Flutter app in separate top-level folders.
- v1.0 admin: a small admin section in the web app for users, invites, quotas and health, plus the command line. No billing, no organizations.
- Sharing between users is by share link only in v1.0; no shared folders between accounts.
- A per-user storage quota with an operator-set default, enforced when an upload starts.
- Observability: structured JSON logs, a Prometheus metrics endpoint and a health endpoint; no bundled monitoring stack.

## Component list

Final list, confirmed 2026-11-04 (`component-decisions.md`) and updated by the Step 4 answers in `canonical-names.md` (which win). Where the Architecture Vision above names components more loosely, this list is the reference.

| # | Component | Crate or runtime unit | What it owns |
|---|-----------|-----------------------|--------------|
| 01 | identity | server library, `sunnycloud` binary | Sign-in (OIDC, GitHub), sessions, device grant, invites (including operator invites with a granted role or a target account), setup token, mobile login codes, account-state guard for commits (`AccountGuard`). Tables `accounts`, `identities`, `sessions`, `device_tokens`, `device_grants`, `invites`, `oauth_states`, `login_codes` |
| 02 | tree | server library, `sunnycloud` binary | Files, folders, revisions, names, conflicts and the commit transaction; `TreeReader` (node lookup, listing, live-file resolution). Tables `nodes` (all columns except `seq`), `revisions` |
| 03 | transfer | server library, `sunnycloud` binary | Upload sessions, blocklists, staged small uploads, batch receipts, quota reservation, download. Tables `upload_sessions`, `commit_receipts` |
| 04 | blobstore | server library, `sunnycloud` binary | Blob layout, encryption, GC, scrub, blob restore, master-key check, `.epoch` marker. Tables `blobs`, `blob_gc`; the blob volume |
| 05 | sync_feed | server library, `sunnycloud` binary | Change feed, long-poll, resync, epoch bump. `account_feeds`, `nodes.seq` |
| 06 | sharing | server library, `sunnycloud` binary | Share links (files only in v1.0). `share_links` |
| 07 | admin | server library, `sunnycloud` binary | Users, invites, account deletion (`DeletionPurger` drives every `AccountLifecycleHook`), health aggregation. No tables |
| 07a | quota | server library, `sunnycloud` binary | Quota ledger: `reserve`, `release_reservation`, `adjust_used`, limits. `quota` |
| 08 | media_jobs | server library, `sunnycloud` binary | Job queue (the queue SQL runs in the API), worker authentication, the `/api/v1/worker-jobs/**` resources, derived files, thumbnail states. `jobs`, `derived`, `worker_status` |
| 09 | worker_supervisor | worker container (`sunnycloud-worker`) | Claims jobs, heartbeats, reads input, writes output and fails jobs over the API worker listener `:8081`; drives decoders; applies the transcode policy from `media-shared`. No tables, no database login |
| 10 | decoder | decoder containers (`decoder-runner`) | Sandboxed libvips and ffmpeg; probes only |
| 11 | http_api | server library, `sunnycloud` binary | Delivery layer: routes, listeners, auth extraction, error mapping |
| 12 | operator_cli | the `sunnycloud` binary | Composition root, janitor loops (no leader: `SKIP LOCKED` or leases), scheduler, rate limiter, operator commands |
| 13 | sync_core | client library (native, WASM) | Sans-IO sync protocol (also the web app's WASM build) plus the client part, including the client-side traits (`SyncClient`, `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink`) |
| 14 | desktop_daemon | desktop binary + its IPC schema crate | Watcher, `LocalIndex`, tray, updater |
| 15 | desktop_settings | desktop binary (Slint) | Settings window |
| 16 | web_app | Svelte 5 app | Browsing, uploads, sharing, admin pages |
| 17 | android_app | Flutter + Kotlin app, `mobile_backup` crate | Camera backup, checkpoint DB |
| 18 | ios_app | v1.1 stub | Reuses `mobile_backup` |
| 19 | vault_core | client library, v1.2 | Vault cryptography |
| 20 | vault_service | server library, v1.2 stub | Server side of the vault. `vaults` (the wrapped file key travels in `revisions.client_meta`) |

Common helpers (server crates, `sc_*`): 01 config, 02 error, 03 clock, 04 audit (owns `audit_log`), 05 ids_hashing, 06 names, 07 db (pool, short transactions, row-level-security scope), 08 ratelimit.

Shared crates: `contracts` (server-only: the port traits that server components call each other through) and `media-shared` (media types, job DTOs, transcode policy and decoder protocol; no sqlx and no queue module; used by 08, 09 and 10).

**Dependency rule.** Every cross-component call goes through a port trait defined in the `contracts` crate. A component depends on `contracts`, never on another component's crate, so the crate graph has no cycles. The commit transaction belongs to 02 tree: it opens the transaction, calls `AccountGuard::ensure_active` (01) first, then the 07a quota port and the 05 sync_feed port inside it (counter last). The per-account advisory lock is taken only by `publish` and folder move; there is no multi-level lock-order contract. `TreeReader` also resolves nodes (it absorbed `NodeResolver`). Account erase runs through the `AccountLifecycleHook` port, implemented by 02, 03, 05, 06, 07a, 08 and 20 and driven by 07's `DeletionPurger`; only 01 triggers hooks. 03 transfer and 08 media_jobs pass their effects to 02 through ports; 02 does not import them. Delivery (11 http_api) only binds and validates, calls one business operation, and maps the result; it holds no SQL and no business rule. 12 operator_cli builds the services (the composition root), including the scheduler and the rate limiter, and passes them to 11 http_api; no component depends on it. Client-side traits live in the client crate `sync-core`, not in `contracts`, because `contracts` pulls server dependencies; `BackupCore` is defined in `mobile_backup` and the daemon control types in `ipc_schema`.

**Open items for Step 4 review** (remaining owner items, not resolved in these documents; see `canonical-names.md`):
- Vault share links beyond v1.2.
- Windows reserved-name marker (`%5F`) and the ` (2)` collision suffix for escaped names.
- Poster-frame SSIM tolerance.
- WASM split in 13: trim (a risk-register item).
- External review of the blob encryption before v1.0.
- Windows signing and the Apple and Google developer accounts.

> See ADR 015 (Ports-only component dependencies), ADR 016 (Staged releases).

## 1. System Context

**Problem being solved**: Operators want a fast, self-hosted alternative to Dropbox that syncs files, photos and videos across web, mobile and desktop folders, with automatic camera backup, media processing, share links and an optional end-to-end encrypted vault, installed on their own infrastructure.

**System boundaries**: Inside: the Caddy proxy, API server, worker processes, PostgreSQL, the blob volume, and all first-party clients. Outside: identity providers, the operator's host, backups and DNS, app stores, and the CI system.

**External systems**:

| System | Integration Type | Direction | Purpose |
|--------|-----------------|-----------|---------|
| GitHub | OAuth2 + REST profile | Both | Sign-in (v1.0) |
| Generic OIDC provider (Google, Authelia, authentik, Keycloak, Pocket ID) | OIDC | Both | Sign-in (v1.0) |
| Sign in with Apple | OIDC with hand-built client secret | Both | Sign-in (v1.1) |
| Google Play / App Store | Distribution | Outbound | Mobile apps |
| GitHub Releases | HTTPS, offline-signed Ed25519 update manifest (two pinned keys) | Inbound | Desktop updater (source fixed at build time) |
| GitHub Actions (fallback git.azaion.com / ci.azaion.com) | CI | Outbound | Build, test, sign |
| Operator backup target | Files / dumps | Outbound | Backups of DB, blobs, master key |

> See ADR 002 (Single-host Compose with one API process).

## 2. Technology Stack

| Layer | Technology | Version | Rationale |
|-------|-----------|---------|-----------|
| Language (server, workers, desktop, core) | Rust | stable, MSRV set by notify 8.2 (1.88) | Owner constraint; performance |
| HTTP server | axum + hyper-util + tokio | axum 0.8.9, hyper 1.12, tokio 1.54.1+ | MVE verified; hand-built accept loop for large HTTP/2 windows |
| Database | PostgreSQL | 18 | Multiple writers; queue; change feed |
| DB access | sqlx | 0.9 | Migrations, SKIP LOCKED |
| Hashing | BLAKE3 | 1.8.7 | Throughput; tree mode |
| Blob encryption | chacha20poly1305 + aead-stream | =0.11.0, =0.6.0 | XChaCha20-Poly1305 STREAM, 64 KiB chunks |
| Identity | openidconnect, oauth2, jsonwebtoken | 4.1.0, 5.0.0 | Built-in OAuth/OIDC client |
| Media | libvips (`vipsthumbnail`), ffmpeg + libx264 | libvips 8.18.7, libheif ≥ 1.22.0 | Subprocesses in sandbox |
| Web | Svelte 5 | current | Owner constraint |
| Mobile | Flutter + Kotlin WorkManager (+ Swift in v1.1) | UniFFI 0.32.2 | Shared Rust uploader; Kotlin (Swift on iOS) owns the uploader handle and sends status to Flutter over a Method/EventChannel; no flutter_rust_bridge |
| Desktop | notify 8.2.1, tray-icon 0.26.1, Slint 1.18.1 (software renderer) + rfd 0.17.2, rusqlite 0.41.1, reqwest 0.13.5 | | MVE verified |
| Proxy | Caddy | current stable | TLS, buffering off, h2c to the API |
| Hosting | Docker Compose on operator hardware | | AC-22 |
| CI/CD | GitHub Actions (fallback Woodpecker on ci.azaion.com) | actions pinned by SHA | Owner constraint |
| Cache / Message queue | none | | Postgres `jobs` table and long-poll; no Redis |

**Key constraints from restrictions.md**:
- Rust for server, workers and daemons: all backend and desktop components are in one Cargo workspace.
- Social login only with controlled registration: the API owns sessions; there are no local passwords.
- Operator hardware: no cloud-only services, so no managed queue, KMS or object store.
- AGPL-3.0 with an App Store exception: all dependencies must be GPL-3.0-compatible.

> See ADR 001 (Rust), ADR 003 (PostgreSQL with plain-file content), ADR 011 (AGPL licence), ADR 012 (Desktop daemon), ADR 013 (Mobile uploader).

## 3. Deployment Model

**Environments**: Development (Compose on a workstation), Staging (stage branch on a shared host, per loop-end setting `stage+main`), Production (operator installs; the project itself runs a demo only if the owner chooses).

**Infrastructure**:
- On-premises or any VM with Docker Compose; no orchestrator in v1.0.
- Scaling: vertical for the API and database; horizontal for workers by adding supervisor containers on the same Compose host. Remote workers on other hosts are out of v1.0: the worker listener `:8081` is plaintext on the internal `backend` network (section 9 answer 7).
- v1.0 runs one API process: upload progress is held in memory, so API replicas are unsupported for uploads. The janitor loops and the scheduler run inside that process (the `sunnycloud` binary).

**Runtime units**:

| Unit | Image or binary | Notes |
|------|-----------------|-------|
| Caddy | `caddy` | On the `proxy` network only (not on `backend`); TLS 1.3, buffering off; proxies to the API at `api:8080` over h2c (HTTP/2 without TLS) |
| migrate | `sunnycloud-api` image, command `sunnycloud migrate` | One-shot Compose step run as `sunnycloud_migrator` before the API starts; the API never migrates itself, and refuses to serve when the schema version does not match |
| api | `sunnycloud-api`, command `sunnycloud serve` | On `proxy` and `backend`; public listener `:8080`; worker listener `:8081` (`/api/v1/worker-jobs/**` only, `backend` network, worker bearer); refuses to start when the database epoch is below the `.epoch` marker and prints `admin epoch-bump`; ops `:9090`; test image only: `POST /test/clock` on `:9091` sets the test-clock offset and `GET /internal/v1/test/clock-offset` on `:8081` reads it (the release image returns 404 for both). One API process in v1.0 |
| worker | `sunnycloud-worker` | Supervisor on `backend`; claims, heartbeats, reads input, writes output and fails jobs over the API on `:8081`; no database login, no blob mount, no master key; `mem_limit` set |
| decoder | `sunnycloud-decoder` (`decoder-runner`) | Two containers per supervisor: `decoder-interactive` (tmpfs scratch with a numeric size cap) and `decoder-general` (disk scratch on a dedicated mount or quota); both `network_mode: none`, reached only through the decoder socket volume. Hardened runner: see section 7 |
| db | PostgreSQL 18 | On `backend`. Two roles: `sunnycloud_migrator` (schema owner, migrations, bypasses row-level security as owner) and `sunnycloud` (API runtime: DML only, no DDL, no `BYPASSRLS`, row-level security enabled, no `FORCE`). The worker and the decoder have no database login |
| operator commands | `sunnycloud` (`docker compose exec|run api sunnycloud ...`) | `serve`, `healthcheck`, `openapi`, `version`, `migrate`, `doctor`, `key-check`, `admin ...` (create, reset, promote, demote, setup-token, invite, user list, user set-quota, user delete, restore, epoch-bump, keycheck-init, rewrap, media-requeue, media-stats, gc-run, audit export, dev-seed), `scrub run [--full]`, `scrub status` |

**Networks**: `proxy` (caddy, api) and `backend` (api, db, worker). Caddy is not on `backend`. The decoders have no network. The test compose uses the same service names.

**Environment-specific configuration**:

| Config | Development | Production |
|--------|-------------|------------|
| Database | Compose Postgres 18 | Compose Postgres 18 with the operator's volume and backups |
| Secrets | `.env` and Docker secrets (test master key) | Docker secrets: master key, provider secrets, Apple `.p8` |
| Durations (retention, TTLs, settle, leases) | Short test values | Production defaults (7 d retention, 10 min codes, 5 min leases) |
| Logging | Console JSON | JSON to the operator's collector |

> See ADR 002 (Single-host Compose), ADR 009 (Sandboxed media pipeline), ADR 017 (Release signing).

## 4. Data Model Overview

**Core entities**:

| Entity | Description | Owned By Component |
|--------|-------------|--------------------|
| Account, Identity | A user and its (provider, id) logins | 01 identity |
| Session, DeviceToken, DeviceGrant | Opaque sessions, sync-only device tokens, pending device approvals | 01 identity |
| Invite, SetupToken, LoginCode | Controlled registration; one-time mobile sign-in codes | 01 identity |
| Node | File or folder in the tree (stable ID, parent, name, name_key, state, tombstone) | 02 tree |
| Revision | A published version of a file node (blocklist, size, hash, blob ref, client metadata) | 02 tree |
| Blob, BlobGc | Stored content object and its scheduled removal | 04 blobstore |
| UploadSession | Open or staged upload with received-bitmap and reserved bytes; batch receipts (`commit_receipts`) | 03 transfer |
| AccountFeed (the feed itself reads `nodes.seq`) | Per-account `epoch.seq` counter | 05 sync_feed |
| ShareLink | Hashed token, file target, expiry, password hash | 06 sharing |
| Quota | Per-account limit and usage | 07a quota |
| Job, Derived | Media tasks and thumbnail/transcode outputs | 08 media_jobs |
| WorkerStatus | Last seen, image age and decoder probe result per supervisor | 08 media_jobs |
| Vault (v1.2) | Vault header; the wrapped file key of a vault file is in `revisions.client_meta` | 20 vault_service |
| AuditLog | Append-only security and admin events (also scrub summaries); the runtime role can only insert and select | helper 04 audit |
| LocalIndex | Desktop SQLite index of local files | 14 desktop_daemon |

**Key relationships**:
- Account → Node: one tree per account; Node → Revision: many, one current.
- Revision → Blob: one blob per revision; Blob → BlobGc once unreferenced.
- Node → Derived: thumbnails and transcodes per revision; Job → Derived.
- Account → feed: changed nodes ordered by `epoch.seq` (`nodes.seq`; no event table).

**Data flow summary**:
- Client → API → blob volume + Postgres: blocks and metadata; one transaction, owned by 02 tree, publishes a revision, applies the quota change (07a) and advances the counter (05) last.
- API → Job → supervisor → decoder → API → Derived: media processing with no worker or decoder access to the database or blobs; the supervisor claims and reports over the API (`:8081`). The supervisor decides the transcode policy from the decoder's probe; the decoder only probes and converts.
- Account erase: 07's `DeletionPurger` drives every `AccountLifecycleHook::erase_batch` (02, 03, 05, 06, 07a, 08, 20) in short batches until all report nothing left; 01 then finalizes (identities, sessions, tokens, grants, login codes, account row).

> See ADR 003 (PostgreSQL with plain-file content), ADR 005 (Last-write-wins), ADR 014 (Two database roles).

## 5. Integration Points

### Internal Communication

| From | To | Protocol | Pattern | Notes |
|------|----|----------|---------|-------|
| Clients | Caddy → API | HTTPS HTTP/2 to Caddy; h2c from Caddy to `api:8080`; `/api/v1` | Request-Response + long-poll | Blocklist protocol, 4 lanes |
| API | PostgreSQL | sqlx, role `sunnycloud` (DML only) | Transactions | Short-lived; counter update last before COMMIT |
| `migrate` step | PostgreSQL | sqlx, role `sunnycloud_migrator` | One-shot before the API starts | Schema owner; the API only checks the schema version |
| 02 tree | 07a quota, 05 sync_feed | `contracts` ports, in the commit transaction | In-process call | Quota change, then counter bump last; 02 owns the transaction |
| API | Blob volume | file I/O (decrypt/encrypt streaming) | Direct | Fdatasync before ack |
| Supervisor | API | worker listener `:8081` (`/api/v1/worker-jobs/**`, `backend` network, no TLS, worker bearer) | Claim (5 min lease, 3 attempts), heartbeat, fetch decrypted input, post output, fail; test image reads the shared clock offset from `GET /internal/v1/test/clock-offset` | No database login, no blob mount, no master key; the queue SQL runs in the API (08) |
| Supervisor | Decoder | Unix socket + scratch dir | Command | Interactive and general decoder containers, both `network_mode: none`; limits per job type |
| Desktop daemon | Settings window | JSON-RPC over user-only socket / ACL'd pipe | Request-Response | No token-returning method |

### External Integrations

| External System | Protocol | Auth | Rate Limits | Failure Mode |
|----------------|----------|------|-------------|--------------|
| OIDC/GitHub/Apple providers | HTTPS | Client secret, PKCE where supported | Provider-defined | 10 s timeout; sign-in fails clearly; existing sessions keep working |
| Operator-configured issuers only | HTTPS | n/a | n/a | No redirects; link-local and metadata ranges blocked |
| GitHub Releases (desktop updater) | HTTPS | Offline-signed Ed25519 update manifest; two public keys (active and recovery) pinned in the build; manifest carries `version`, per-OS SHA-256, a monotonic `serial` and an `expires` time (exact layout is an ADR) | n/a | Update skipped, daemon keeps running |

> See ADR 004 (Blocklist sync), ADR 008 (OIDC sign-in), ADR 009 (Media pipeline), ADR 015 (Ports-only dependencies).

## 6. Non-Functional Requirements

| Requirement | Target | Measurement | Priority |
|------------|--------|-------------|----------|
| Throughput | ≥ 90% of 1 Gbps single transfer | NFT-PERF-02/03 | High |
| Sync latency | ≤ 5 s p95, ≤ 10 MB | NFT-PERF-08/09 | High |
| Listing | ≤ 300 ms p95, 10,000 entries | NFT-PERF-10 | Medium |
| Scale | 500 users, 100 concurrent sessions | NFT-PERF-01 | High |
| Data retention | 7 days blob retention (0-90), 90 days tombstones | FT/NFT scenarios | High |
| Recovery | Documented restore of DB + blobs + master key; devices resync; `serve` refuses to start and `doctor` reports Fail when the database epoch is below the `.epoch` marker on the blob volume (missing epoch bump) | NFT-RES (AC-41, AC-65) | High |
| Durability | No zeroed blocks after `kill -9`; fdatasync before ack | NFT-RES-03 | High |
| Scalability | Add workers to raise transcode throughput (≥ 1.7x for 2 workers) | NFT-PERF-17 | Medium |

> See ADR 004 (Blocklist sync), ADR 010 (CPU-first transcoding).

## 7. Security Architecture

**Authentication**: social login through OIDC/OAuth; server-issued opaque sessions (SHA-256 hashed at rest) with refresh rotation and 30 s reuse grace; sync-only device tokens through the server's device grant; mobile apps exchange a one-time login code (stored hashed in `login_codes`, PKCE-bound) for a session.

**Authorization**: every query scoped by account; generated two-account IDOR matrix over the OpenAPI (AC-59); Postgres row-level security only as defence in depth; admin role for operator functions; the last admin cannot be demoted, disabled or deleted. Two database roles: `sunnycloud_migrator` (schema owner, migrations, bypasses row-level security as owner) and `sunnycloud` (API runtime: DML only, no DDL, no `BYPASSRLS`, policies enabled without `FORCE`). The worker and the decoder have no database login; workers authenticate to the API with a worker bearer checked only by 08 (`WorkerAuthenticator`).

**Data protection**:
- At rest: per-blob keys wrapped by an operator master key (XChaCha20-Poly1305 STREAM, 64 KiB chunks); thumbnails and transcodes included; volume encryption recommended in addition.
- In transit: TLS 1.3 at Caddy, HSTS one year.
- Secrets management: Docker secrets or files; never in git; master key backed up separately.
- Updates: the manifest is signed offline (or in a manual job), never by an automatic CI secret. The desktop daemon pins two Ed25519 public keys (active and recovery) and accepts only a manifest from the GitHub Releases source fixed at build time that verifies against one of them, has a `serial` above the last accepted one, has not passed its `expires` time and matches the per-OS SHA-256; unsigned, other-key, lower-serial and expired manifests are rejected. Releases are signed only from the GitHub release workflow; the Forgejo/Woodpecker fallback builds and tests only.
- Decoder isolation: libvips and ffmpeg run in containers with `network_mode: none`, no credentials, no blob mount and no master key. The runner is hardened: `PR_SET_DUMPABLE=0`, child subreaper, SIGKILL of all descendants after every job, seccomp denying `ptrace` and `process_vm_*`. The runner exits after any hostile-class outcome and Compose restarts it. No gVisor in v1.0.
- Epoch: `serve` refuses to start when the database epoch is below the `.epoch` marker, so a restore without `admin epoch-bump` cannot serve stale cursors.
- Vault (v1.2): client-side XChaCha20-Poly1305 with Argon2id; the key-wrap AAD binds vault id, file id and format version (not the parent); server sees only ciphertext.

**Content serving**: allowlisted image and video types inline with `nosniff`; everything else as attachment with sandbox CSP.

**Audit logging**: sign-ins, device approvals and revocations, share create/revoke/access, vault failures, admin actions, account deletion, scrub mismatches and scrub run summaries (summaries are audit events plus metrics, not a table); append-only (the runtime role `sunnycloud` has INSERT and SELECT only and owns nothing, so a compromised API process cannot update, delete or re-grant; retention purge is a `SECURITY DEFINER` function owned by `sunnycloud_migrator`; a CI test checks it), IDs only, retention set by the operator.

> See ADR 006 (Blob encryption), ADR 007 (Vault), ADR 008 (OIDC sign-in), ADR 009 (Media pipeline), ADR 014 (Database roles), ADR 017 (Release signing).

## 8. Key Architectural Decisions

Full records are captured in `adr/` (Step 4.5).

- **ADR-001 Rust for server, workers and desktop** — maximum performance, one shared core for Svelte (WASM) and Flutter (UniFFI).
- **ADR-002 Single-host Compose with one API process** — workers reach only the API; remote workers are out of v1.0.
- **ADR-003 PostgreSQL metadata with plain-file content** — ID-named blobs, `jobs` queue in Postgres, no extra service.
- **ADR-004 Blocklist sync over HTTP/2 with a server-ordered feed** — 2 MiB blocks, `epoch.seq` cursor, long-poll.
- **ADR-005 Last-write-wins with conflict copies** — edit beats delete, 7-day operator-only retention, no user trash.
- **ADR-006 Application-level envelope encryption of blobs** — owner decision; costs zero-copy sending.
- **ADR-007 Opt-in client-side vault (v1.2)** — end-to-end encryption only where the owner asks for it.
- **ADR-008 Built-in OIDC sign-in and device grant** — rejected Dex/Keycloak; Apple has no device flow.
- **ADR-009 Sandboxed media pipeline with API-only workers** — contains decoder compromise; no worker database login.
- **ADR-010 CPU-first transcoding with opt-in GPU** — transcode only when not compliant; NVENC in v1.1.
- **ADR-011 AGPL-3.0 with App Store exception** — worker image ships GPL ffmpeg/x264 with a source offer.
- **ADR-012 Native desktop daemon with a separate Slint settings process** — Tauri is the fallback.
- **ADR-013 Shared Rust uploader behind UniFFI for mobile** — no flutter_rust_bridge.
- **ADR-014 Two database roles with row-level security** — migrator owns the schema; the API role is DML only.
- **ADR-015 Ports-only component dependencies** — `contracts` traits; the tree owns the commit transaction.
- **ADR-016 Staged releases v1.0 / v1.1 / v1.2** — contains schedule risk; vault after audit.
- **ADR-017 Release signing and offline updater keys** — GitHub Actions signs; the fallback CI only builds.

## 9. Confirmed Design Answers (2026-11-04)

> Superseded in part at plan Step 4 (owner answers, see `canonical-names.md`): workers claim jobs through the API (`/api/v1/worker-jobs/**`), not from the database queue; there are two database roles (`sunnycloud_migrator`, `sunnycloud`); the epoch bump command is `sunnycloud admin epoch-bump`. The answers below are kept as originally confirmed.

Confirmed by the owner after the first drafts of the flows, data model and deployment plan.

1. **Epoch after a restore** — a random epoch id written by an explicit `sunnycloud epoch-bump` step in the restore procedure; a cursor above the current head also triggers `resync_required`; `sunnycloud doctor` warns if the bump is missing.
2. **Conflict copy** — the superseded revision moves to a new node sharing the same blob, named `name (conflict copy YYYY-MM-DD HH-MM device)`.
3. **Thumbnail lanes** — a job is bulk when the client marks it as an initial import or camera-roll first sync, or when the account already has more than 200 queued jobs; all others use the reserved interactive lane.
4. **Quota** — counts only current revisions of live files; thumbnails and transcodes do not count.
5. **Thumbnail readiness in the web app** — listings carry a thumbnail state; the page polls pending items every 2 s while visible.
6. **Mobile sign-in** — system browser plus a one-time code returned through a custom scheme or universal link (PKCE); the device grant is for desktop only.
7. **Supervisor to API** — a per-worker secret bearer token over the private internal network; no TLS inside the Compose network.
8. **Supervisor to decoder** — the decoder container runs a long-lived loop on a Unix socket in a shared volume, one job at a time, with a shared scratch area; no Docker socket is exposed.
9. **Master-key check** — a key-check value is stored at first start so a wrong master key fails closed.
10. **Data model** — no change-event table (the feed reads `nodes.seq` and returns the latest state of each changed node); the setup token is an invite with kind `setup`; derived files are keyed by account and content hash; the job unique key adds the account.
11. **Data model assumptions** — names up to 255 bytes; upload sessions expire after 24 h of inactivity (rolling); device tokens do not rotate in v1.0; the orphan sweep deletes by default.
12. **Camera backup** — per-revision client metadata (device, MediaStore asset id, link to original).
13. **Vault failures** — the client reports wrong-passphrase failures best-effort so the audit log can record them.
14. **Delivery** — GHCR only, amd64 images only, SignPath Foundation for Windows signing in v1.0.
