# MVP roadmap: Linux server and web client

Written 2026-11-20, updated the same day after the owner's answers. Target: 4 working days (3 was the first wish; the
WebSocket channel and the full preview pipeline add a day), budget 1 week. Nothing valuable is dropped: every cut below is
either postponed with its proper form named, or built in a simpler form now and replaced after the MVP.

## 1. The MVP in one paragraph

A person opens `https://drive.zxsanny.com` in a browser, signs in with Google, and sees their files and photos as a grid.
They pick files, photos and videos, upload them, and see each one again with a static preview: a thumbnail in the grid, a
larger image on click, a poster frame for a video. Files uploaded in one browser appear in another **without a reload,
pushed over a WebSocket** that carries the changes themselves (binary frames packed with `packbin`). A file can be
downloaded. There is no playback. The server runs on zxstation (Linux) and is reachable from the internet with a
Let's Encrypt certificate.

**Out of the MVP, by the owner's decision:** the native iOS app. The owner has a Mac with Xcode and an Apple developer
account; the iOS tickets are done later in a session on that Mac (section 7).

## 2. Reading of the request (confirm or correct)

- **Sync means upload and see everywhere**, not two-way folder sync. Folder sync is the desktop agent's job (E-24) and is postponed.
- **Previews are a real pipeline**, not a throw-away one: section 5B.
- **Realtime from the first version:** the web client gets live state as pushed data frames over one WebSocket, in both
  directions, and has no timer that asks the server "anything new?" (global rule; section 5E).

## 3. What exists and what is missing (checked in the code on 2026-11-20)

| Area | State |
|---|---|
| Server API (`sunnycloud serve`): sign-in, tree, small and large upload, download with ranges, change feed (long poll), thumbnails, shares | Built and wired to real implementations; never run end to end by me yet |
| WebSocket | Does not exist anywhere in the server (axum 0.8.9 is used; the `ws` feature is not enabled) |
| Web app | Does not exist. No `src/web`, no `package.json`. All 19 E-26 tickets are open |
| Serving the SPA | `with_spa` exists in 11_http_api but `serve` never passes a bundle; the dev Caddyfile only proxies the API |
| Sign-in | No password login. The generic OIDC provider is documented as "Google included" (discovery, PKCE, ID-token checks); startup is refused without a provider. The `mock-idp` named in the compose files does not exist |
| First user | `sunnycloud admin setup-token`, then sign in with the token (not printed at start, despite the docs) |
| Browser session | Cookie lives as long as `access_token_ttl` (15 min default); the refresh route sets no cookie |
| Previews | Not working. The decoder answers every job `Unsupported` (`NoExecutors`); its image has no libvips, libheif or ffmpeg |
| Browser uploads | Every upload, even a 1 KB file, needs a BLAKE3 hash per body or 2 MiB block, from `sync-core-wasm`; the WASM is not built for the web yet |
| Dev compose | Test-oriented: test-clock images, 30 s retention, 60 s upload sessions, localhost-only TLS |
| `packbin` | The owner's library, `/develop/packbin`; published as `packbin` 0.5.1 on crates.io and npm (the local checkout says 0.1.0: the pin is verified against the registry source before use). Rust and TypeScript both exist |
| Host zxstation | 24 cores, 60 GB RAM, ffmpeg, vips, libheif present; Docker (rootful); ports 80 and 443 free; 517 GB free on `/develop` (71% used); no Caddy binary, no pnpm |
| DNS | Zone `zxsanny.com` is on Cloudflare. `drive.zxsanny.com` exists and resolves to **Cloudflare proxy addresses** (104.21.x, 172.67.x), not to 95.47.104.30: the orange-cloud proxy is on. Port forwarding to zxstation is enabled (reported by the owner; proven by the first certificate issue) |

## 4. Decisions

| # | Decision | Taken | Cost |
|---|---|---|---|
| D1 | Sign-in provider | **Google through the generic OIDC provider** (issuer `https://accounts.google.com`). The owner creates one OAuth client in Google Cloud (section 8). The server has no passwords, so some identity provider is unavoidable | Google "testing" mode allows named test users only, which fits an MVP; publishing the consent screen later is a Google form |
| D2 | Who serves the SPA | **Caddy** serves the built files and proxies `/api` (the design in `containerization.md`) | A later move into `serve` is a few lines if wanted |
| D3 | How processes run | **Host processes** for api, worker and decoder (uses this host's vips, ffmpeg, libheif); **Docker** only for Postgres and Caddy. `scripts/dev/mvp-up.sh` starts and stops it | The decoder container with a media toolchain stage (AZ-3863, AZ-3864) comes after the MVP |
| D4 | Certificate | **Let's Encrypt through Caddy's built-in ACME** for `drive.zxsanny.com` (HTTP-01 on port 80), as the project's deployment docs specify. `caddy_data` persists. The account email lives in `secrets-dev/`, not in git. The sibling repos (Archangel) use host nginx with certbot or acme.sh behind a DNS plugin, which fits a host that already runs nginx; zxstation does not. The first test runs against the Let's Encrypt staging CA | Needs ports 80 and 443 reachable from outside |
| D5 | Realtime | **WebSocket from the first version, `packbin` frames** (section 5E) | About a day more than the long-poll shortcut. The long-poll `/changes` stays for desktop and mobile clients until they move over (they are later stages); web uses only the WebSocket |
| D6 | Session length | `access_token_ttl` set to 12 h for the MVP | Proper cookie renewal after the MVP (the refresh route must set the cookie) |
| D7 | Public exposure | The first account is created by the setup token: run the setup right after the first start and keep the token secret. After that, accounts need an invite (CLI). Rate limits and CSRF checks already exist in the API. A short exposure review (day 1) comes before the first public start | |
| D8 | Cloudflare proxy | **Recommended: set `drive` to "DNS only" (grey cloud).** The proxy is on today. Behind it: every visitor appears with a Cloudflare address, which breaks the per-IP rate limits and the abuse log unless Caddy is told to trust Cloudflare's ranges; the CDN terms of the free plan restrict serving large media files; the request size limit is 100 MB per request (the 2 MiB upload blocks fit); WebSocket works but idle connections are cut after about 100 s (a 30 s ping covers it) | If you keep the proxy: Caddy `trusted_proxies` with Cloudflare's ranges and `CF-Connecting-IP`, SSL mode "Full (strict)", about half a day more |

## 5. Work packages

Four packages run in parallel from day 1. Each owns disjoint paths. I commit and push.

### A. Stack, certificate and sign-in (owns `deploy/`, `scripts/dev/`, `secrets-dev/`)
- Real-use compose for Postgres 18 and Caddy on ports 80 and 443: `PUBLIC_HOST=drive.zxsanny.com`, ACME with the account email, staging CA switch, `caddy_data` volume, `src/web/dist` served, `/api` proxied, WebSocket upgrade proxied (check against the `h2c` upstream setting on day 1).
- Secrets, migrations, key check, config with real TTLs (retention, upload session 24 h, `access_token_ttl` 12 h, `public_url=https://drive.zxsanny.com`).
- Google OIDC provider wired; `admin setup-token`; the `/setup` flow proven end to end with curl before the UI exists.
- `scripts/dev/mvp-up.sh`, a README section, a disk-space check.

### B. Previews (owns `src/decoder/`, `src/worker/`, `src/media-shared/`)
The real pipeline, host binaries, no container stage yet:
- AZ-3857 image executor: `vipsthumbnail` with libheif (JPEG, PNG, WebP, HEIC), two WebP sizes (320 px thumb, 1440 px preview).
- AZ-3858 probe and AZ-3859 video poster: `ffprobe` and `ffmpeg`, one poster frame.
- AZ-3854 and AZ-3855 process containment: decoding hostile files is a security boundary, so the guard and kill-all go in now.
- Every failure ends as a `failed` state, not a hang; a 1 GB video does not wedge the worker. Tested with real HEIC and MOV files.
- Postponed with their proper form: AZ-3860 transcode, AZ-3862 NVENC, AZ-3861 self-check, AZ-3863 and AZ-3864 container media stages, AZ-3865 and AZ-3866 suites and performance.

### C. Web app (owns `src/web/`, the new web job in `.github/workflows/ci.yml`)
Svelte 5, Vite, TypeScript, a client generated from `/api/v1/openapi.json`, the layout of `module-layout.md`, `packbin` from npm.

| Ticket | MVP form | Postponed proper form |
|---|---|---|
| AZ-3935, AZ-3956 scaffold, tokens, shell | Yes, the real skeleton: router, generated client, error type, layout shell, design tokens from `ui_mockups/DESIGN.md` | Full component library |
| AZ-3936 sign-in and first-run pages | Yes (Google button) | Provider matrix polish |
| AZ-3938 file browser | List and photo grid, folder navigation, create folder, delete, cursor paging; updates arrive over the WebSocket | Rename, move, virtual scrolling at 100k |
| AZ-3939 small-file upload | Yes: staged PUT with the hash, batch commit | Batch limits UI |
| AZ-3940 large upload in a Worker | Yes: 2 MiB blocks hashed by `sync-core-wasm`, 4 lanes, progress, cancel, retry from the start | AZ-3941 resume across reloads (IndexedDB), edit detection |
| AZ-3942 download | A plain link to `/nodes/{id}/content` | Verified downloads, OPFS path, labels |
| AZ-3945 previews | Thumbnail in the grid, 1440 px image on click, video poster only, placeholder on `failed`. **Thumbnail state comes as pushed frames; the 2 s polling in the design is dropped** | Polishing of states |
| AZ-3947 render modes | Yes (security rule: inline only for safe types) | |
| AZ-3937 account page, AZ-3943 shares, AZ-3944 device approval, AZ-3946 admin, AZ-3948, AZ-3954 vault, AZ-3950, AZ-3952 | Not in the MVP. Admin and invites use the CLI | Built in the order of section 7 |

The web CI job runs lint, typecheck, unit tests and build, so CI stays green.

### E. Realtime channel (owns the new `src/components/11_http_api/src/realtime/`, `src/components/05_sync_feed/` adapter code, the contract doc `_docs/02_document/contracts/11_http_api/realtime.md`)
- Endpoint `GET /api/v1/realtime`, a WebSocket upgrade. Authenticated by the session cookie; the handshake `Origin` must equal `public_url` (protection against cross-site WebSocket hijacking); a per-account connection cap.
- Binary frames, one `packbin` scheme per message type, the first byte selects it. Both sides keep the same field list; the list lives in the contract doc and is tested byte for byte from both languages (Rust server, TypeScript client).
- Frames carry the data, never a "changed, read again" signal. Client to server: `Subscribe{epoch, seq}` (resume point), `Ping`. Server to client: `Hello`, `Change{...}` (the changed node with its fields, revision, size, media type and thumbnail state, or a deletion), `ThumbState{...}` when a preview lands, `Reset` (resume point too old: the client reloads the listing once over REST, which the rule allows as first load), `Pong`.
- Source of the pushes: the server's `InProcessNotifier` wakes a per-connection task, which reads the delta from the existing change-feed repo and sends frames. REST stays for first load, writes and the public API. The thumbnail-ready push needs a hook where the worker's result is stored; resolved on day 2.
- Heartbeat: a 30 s `Ping`/`Pong` keeps idle connections alive through any proxy (a keepalive, not polling).
- Tests with a real Postgres: reconnect and resume, two connections of one account, Origin refused, cap reached, frames decode in both languages.
- `packbin` pinned exactly in both lockfiles; its API verified at that version; `cargo deny` and the npm audit stay green.

### D. Acceptance (shared, day 4)
A script-driven run on zxstation with a mixed media set (JPEG, HEIC, PNG, MOV, MP4, one video over 1 GB): upload in one
browser, previews appear, a second browser sees the files appear live, download matches the original hash. A Playwright
smoke test (sign-in, upload, preview) joins CI.

## 6. Schedule (4 days, each day ends with a check you can run)

**Day 1, foundation.**
- A: stack up on the Let's Encrypt staging CA, Google sign-in, setup token proven with curl; exposure review before the first public start; WebSocket proxy check.
- B: image executor; a HEIC and a JPEG sent through the API come back as thumbnails.
- C: scaffold, generated client, sign-in and setup pages, SPA served by Caddy.
- E: the realtime contract (frames, fields) written and reviewed with you; endpoint skeleton with `Hello` and `Subscribe`.
- *Check:* at `https://drive.zxsanny.com` you sign in with Google and land on an empty file browser.

**Day 2, the core.**
- C: file browser, small and large uploads with WASM hashing, grid with thumbnails, preview on click.
- B: video poster, probe, containment, failure states.
- E: live `Change` and `ThumbState` frames, resume and `Reset`.
- A: production Let's Encrypt CA, start and stop script, README.
- *Check:* upload 20 mixed files from one browser; they appear in a second browser without a reload, each with a preview.

**Day 3, integration.**
- C: the realtime client wired into the grid and the upload progress; error states.
- All: end-to-end fixes; the 1 GB video; reconnect after the server restarts.
- *Check:* kill and restart the server while a browser is open; the page reconnects and catches up by itself.

**Day 4, hardening and acceptance.**
- D: the acceptance run; fix what it finds.
- Tests in the project's AAA style for new Rust code, the Playwright smoke test, docs, green CI.
- *Check:* 50 photos and 5 videos from your devices appear with previews in another browser.

Days 5 to 7 are slack. The most likely overruns, in order: the first end-to-end run of the real stack (it has never run),
the WebSocket proxy path through Caddy and Cloudflare, the WASM build inside Vite, HEIC and MOV edge cases, and large-file
memory in browsers.

## 7. After the MVP, in this order

1. Resume across reloads and edit detection (AZ-3941), verified downloads (AZ-3942), cookie renewal (D6), move desktop and mobile clients from the long-poll to the realtime channel so one pipeline remains.
2. Share links and the public page (AZ-3943), account page, devices and sessions (AZ-3937, AZ-3944), admin UI (AZ-3946).
3. **Native iOS app, in a session on the owner's Mac** (E-28: AZ-3974 to AZ-3984 and AZ-4019, the UniFFI static library AZ-3975 needs Xcode). Server prerequisite: Sign in with Apple provider (AZ-4024), required by App Store rule 4.8; device grant and mobile code exchange already exist.
4. Hardened decoder container, transcode, NVENC (the rest of E-19), the E-31 black-box stack and the pipeline suites.
5. Android app (E-27), desktop agents (E-24, E-25), macOS agent, then the vault (v1.2 after the external review).

## 8. What I need from you

- **Cloudflare, day 1:** switch `drive.zxsanny.com` to "DNS only" (grey cloud) as D8 recommends, or tell me to keep the proxy and take the extra half day.
- **Google OAuth client, before the sign-in check on day 1:** in Google Cloud Console, APIs and Services, OAuth consent screen: External, testing, add your Google account as a test user. Then Credentials, Create credentials, OAuth client ID, type "Web application", authorized redirect URI `https://drive.zxsanny.com/api/v1/auth/providers/oidc/callback`. Put the client id and secret where I tell you (`secrets-dev/`, not in git). I give the exact file names when the stack exists.
- Corrections to section 2 and the decisions in section 4.
