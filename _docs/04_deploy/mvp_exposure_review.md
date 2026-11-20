# MVP public-exposure review: facts

Date 2026-11-20, host zxstation, `https://drive.zxsanny.com`. Facts only, each with its source: a command run against the live stack, or a file in the repository. Paths are relative to the repository root. Stack and operation: `mvp_runbook.md`.

## 1. Listening sockets (`ss -ltnp`, stack running)

| Address | Process | Reachable from |
|---|---|---|
| `*:80`, `*:443` (tcp, IPv4 and IPv6) | Caddy container, `network_mode: host`, admin API off (`deploy/compose/Caddyfile.mvp`) | everything that can reach the host; router forwards 80 and 443 (owner) |
| `127.0.0.1:8080` | `sunnycloud serve`, public listener | local processes; Caddy |
| `127.0.0.1:8081` | `sunnycloud serve`, internal (worker) listener, bearer token | local processes |
| `127.0.0.1:9090` | `sunnycloud serve`, ops (`/healthz`, `/readyz`, `/metrics`) | local processes, no authentication |
| `127.0.0.1:9092` | `sunnycloud-worker`, ops | local processes |
| `127.0.0.1:5532` | Postgres 18 (docker, published on 127.0.0.1 only) | local processes with the role password |
| unix sockets `sunnycloud-data/run/decoder-{interactive,general}.sock` | decoder-runner, mode 0660 (`src/decoder/src/socket.rs`) | the host user and its group |
| no UDP 443 | HTTP/3 is off | |

Not part of the stack but on the same host (seen in the same `ss` run): `0.0.0.0:22` (sshd), `*:3389` (krdpserver), and ports 35570 to 35600 on `0.0.0.0` and `[::]`, each a throw-away `postgres:18-alpine` container that a test run published (`docker ps` shows `0.0.0.0:355xx->5432/tcp`). They come and go with test runs. They are LAN-reachable and are not forwarded by the router (owner: only 80 and 443).

## 2. What the internet can reach

Through Caddy only. `drive.zxsanny.com` currently resolves to Cloudflare proxy addresses (104.21.x, 172.67.x, 2606:4700::), not to the host (D8 in `_docs/02_document/mvp_roadmap.md`). A request by IP address without SNI gets no certificate (`curl -k https://127.0.0.1/` fails the handshake: exit 000).

- Port 80: HTTP-01 challenges and a 308 redirect to HTTPS (`curl -I http://127.0.0.1/ -H 'Host: drive.zxsanny.com'`).
- Port 443: TLS 1.3 only (TLS 1.2 gets `protocol version` alert 70), ALPN `h2`, certificate for `drive.zxsanny.com` only. Currently a Let's Encrypt STAGING certificate (issuer `(STAGING) Artificial Amaranth YE1`).
- `/api/*` goes to `127.0.0.1:8080` (`Caddyfile.mvp`): the 64 public-listener operations (72 with the 8 worker ones) of the OpenAPI document `GET /api/v1/openapi.json`. Auth class counts of that document: SessionOrDevice 29, SessionOnly 12, Admin 7, ShareGrant 4, DeviceOnly 1, **Public 11**, Worker 8 (listener `internal`, not routed).
- Public (no credential) operations: `POST /auth/login-attempts`, `POST /auth/mobile-code-exchanges`, `GET /auth/providers`, `GET /auth/providers/{provider}/callback`, `POST /auth/refresh-tokens`, `POST /device-grants`, `POST /device-tokens`, `GET /openapi.json`, `GET /public-shares/{token}`, `POST /public-shares/{token}/password-verifications`, `GET /server-info` (all under `/api/v1`).
- Worker routes (`/api/v1/worker-jobs/**`) exist only on the internal listener; through Caddy `GET /api/v1/worker-jobs/status` answers 404. `/readyz`, `/metrics`, `/healthz` through Caddy return the SPA index (200, HTML), not the ops handlers.
- Everything else is the SPA folder (`/srv/web`, read-only mount) with SPA fallback to `index.html`, carrying the AZ-4021 CSP (`default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`), exactly one CSP header on `/` and `/files/x`. Until `src/web/dist/index.html` exists the folder is `deploy/mvp/web-placeholder`.
- Request body limit at the edge 64 GB; the API applies its own ceilings per body class (small 64 KiB, commit and batch 4 MiB, block 2 MiB; `src/components/11_http_api/src/limits/plan.rs` tests).
- Response headers set by the API and seen through Caddy: `Strict-Transport-Security: max-age=31536000` (no `includeSubDomains`, no preload), `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store`.

## 3. Authentication, cookies, CSRF, rate limits

- No passwords exist. Sign-in is the configured OIDC provider (Google). A new account is created only by redeeming an invite or the setup token: `register` returns `InvitationRequired` without a token (`src/components/01_identity/src/sign_in/register.rs:43`). Sign-in of an unknown identity without a token creates nothing.
- Cookies (`src/components/11_http_api/src/registry/signin/cookie.rs:14`): `__Host-session` and `__Host-signin-binding`, attributes `Secure; HttpOnly; SameSite=Lax; Path=/`, no `Domain`. The session cookie has `Max-Age` equal to `access_token_ttl` (12 h here); the binding cookie has no `Max-Age`. The refresh route sets no cookie (roadmap D6), so a browser session ends after 12 h.
- Share-grant cookie `__Secure-share-grant`: `HttpOnly; Secure; SameSite=Lax; Path=/api/v1/public-shares/<token>` (`registry/sharing/grant.rs:60`).
- CSRF (`src/components/11_http_api/src/csrf/mod.rs`): a cookie-authenticated POST, PUT, PATCH or DELETE needs `Sec-Fetch-Site: same-origin` or an `Origin` equal to `public_url` (never the `Host` header); otherwise `csrf_rejected`. Bearer-authenticated calls are not checked. GET and HEAD are never checked, so a WebSocket upgrade (GET) has no CSRF guard from this layer: the realtime endpoint must check `Origin` itself (roadmap package E).
- Sign-in begin and complete check the request `Host` against `public_url` (`sign_in/mod.rs:300`).
- Tokens (setup, invite, session): prefix plus 256 random bits, stored as hashes (`src/shared/ids_hashing/src/token.rs:143`).
- Rate limits are in memory, per process, reset on restart (`src/shared/ratelimit/src/lib.rs`): sign-in failures 10 per minute per IP; invalid invite or setup tokens and device grants 100 per minute per IP (`src/server/src/root/base.rs:22`); device approval 5 per hour per session, 20 per hour per account, 50 per hour per IP; share password 10 per 10 minutes per link; invalid share tokens 100 per minute per IP. There is no generic per-route limiter in v1.0.
- Client address: the API trusts `X-Forwarded-For` only from `SUNNYCLOUD_TRUSTED_PROXIES=127.0.0.1/32` (Caddy) and Caddy replaces the header with `{remote_host}` (`Caddyfile.mvp`). With the Cloudflare proxy on, `{remote_host}` is a Cloudflare edge address, so every visitor behind the same edge shares one per-IP bucket and the abuse log names Cloudflare addresses. The first staging issue logged validator requests from 104.22.x, 172.71.x, 162.158.x and 172.68.x addresses.
- Cloudflare terminates the visitor's TLS while the proxy is on; it sees file bodies in clear. Blob encryption (per-blob key under the master key) applies at rest on this host only.

## 4. Before the first admin exists, and the setup token window

- `GET /api/v1/auth/providers` answers `{"providers":[{"key":"oidc",...}],"setup_required":true}`.
- The setup token is created only by the CLI `sunnycloud admin setup-token` (`src/components/01_identity/src/invites.rs:170`); the server does not generate or print one at start (contrary to `environment_strategy.md`). It is stored hashed, one open at a time (a new call deletes the older one), valid `setup_token_ttl` = 7 d, and `replace_setup_token` refuses when any account exists (`operator_repo.rs:45`). It is redeemable only while no account exists (`registration_repo.rs:106`).
- Today a token is open in the database (created while proving the flow; its value was not stored or printed). Running the command again replaces it.
- What redeeming needs: the token, a successful Google sign-in at the configured client, and (when `SUNNYCLOUD_ALLOWLIST_EMAILS` is set) a verified email on that list. **The allowlist is not set**, so the first Google identity that presents a valid token becomes admin. The token has 256 random bits; guessing is blocked by the 100 per minute per IP counter and by the entropy; the exposure is a leak of the printed token, not an online guess.
- Google sign-in is not usable yet: `oidc_client_id` and `provider_oidc_client_secret` are placeholders. `POST /api/v1/auth/login-attempts` with a setup token already answers 201 with a Google authorization URL (PKCE S256, `redirect_uri` `https://drive.zxsanny.com/api/v1/auth/providers/oidc/callback`, scope `openid email profile`) and sets the binding cookie; Google would reject the placeholder client.
- An unauthenticated caller can create sign-in state rows (`login-attempts`), bounded by 10 per minute per IP and expiring after `oauth_state_ttl` (10 min).

## 5. Disk

- `/develop` (blob root, `sunnycloud-data`, docker root `/develop/docker` with the Postgres volume): 1.8 T, 452 GiB free (75% used) at review time. One filesystem for blobs, database and certificates.
- Free-space floor: `SUNNYCLOUD_BLOB_FREE_RESERVE=20GiB`; the effective floor is the smaller of that and `blob_free_reserve_percent` (default 5%) of the volume (`src/components/04_blobstore/src/config.rs:20`). `doctor` reports the floor as 21474836480 bytes. Below it uploads and upload sessions are refused (`FreeSpaceFloor`, `src/components/03_transfer/src/upload.rs:388`); `mvp-up.sh` also refuses to start below 50 GiB.
- Other limits: per-account quota `default_quota` 100 GiB, `max_declared_size` 50 GiB, `max_open_sessions` 8, `max_staged` 1000, `upload_session_ttl` 24 h.
- Quota times accounts can exceed the disk: quota is not a reservation, only the floor protects the volume.

## 6. Processes and secrets

- api, worker and both decoders run as the login user `zxsanny`, the same user that owns `secrets-dev/` (master key, database URLs, worker token) and the blob files. The decoder is the component that parses hostile media; as a host process it has the same filesystem access as the API. Its isolation is the containment of AZ-3854 and AZ-3855 (package B), not yet part of the stack at review time. The decoder switches off core dumps (which also hides `/proc/<pid>/exe` from its own user).
- Secret files: `secrets-dev/` mode 700; `postgres_password`, `db_api_url`, `db_migrator_url` 644 (the db container reads them as uid 999; the directory mode keeps other host users out); the rest 600. Gitignored (`.gitignore`: `secrets-dev/`).
- The API refuses to start with a missing, empty or wrong-length master key and with a key that fails the key check (`src/server/src/serve/start.rs`). The master key exists once, on the same disk as the data; there is no backup of it or of the database configured.
- Database roles: `sunnycloud_migrator` (DDL, used only by `sunnycloud migrate`) and `sunnycloud` (DML only), created by `deploy/postgres/initdb.d/10-roles.sh`; both can log in over `127.0.0.1:5532`.
- Caddy container: `cap_drop ALL`, `cap_add NET_BIND_SERVICE, DAC_READ_SEARCH`, `no-new-privileges`, host network, config and bundle mounted read-only, certificates in the `caddy_data` volume.
- The release binary is built without the `test-clock` feature (`mvp-up.sh` builds with no features); environment is `production`, so the TTL floors of `config` apply.

## 7. Open risks

1. Cloudflare proxy on: per-IP limits and abuse log see Cloudflare addresses; Cloudflare sees traffic in clear; a 100 MB request cap and idle WebSocket cut-off at about 100 s apply (roadmap D8).
2. No email allowlist: a leaked setup token (or later invite) is redeemable by any Google identity.
3. Setup token is printed to a terminal and lives 7 days; it is open now.
4. Decoder runs with the API's user and file access (host process); media parsing is the main attack surface.
5. Master key, database and blobs are on one disk with no backup; losing `secrets-dev/master_key` loses all data.
6. Session lifetime 12 h with no renewal (D6): the cookie is the only credential of a browser; stolen, it works until expiry. HttpOnly and `__Host-` limit theft by script.
7. WebSocket endpoint does not exist yet; its handshake is not covered by the CSRF layer (GET), and the accept loop uses `serve_connection` without upgrades (`src/components/11_http_api/src/accept.rs:182`), which hyper needs `serve_connection_with_upgrades` for.
8. Rate-limit counters reset on API restart; no limit exists on authenticated routes.
9. The ops endpoints (`/metrics`, `/readyz`) on loopback have no authentication; any local user can read them. Postgres on 127.0.0.1:5532 is open to any local user with the password files (644 for the three container-read files: `db_api_url` holds the API role password).
10. Test containers on 0.0.0.0 (section 1) are LAN-reachable.
11. Disk full risk: floor 20 GiB on a disk that also holds Postgres (WAL) and certificates; accounts can each take 100 GiB.
12. Staging certificate: browsers reject it until `ACME_CA` is switched (runbook); the production switch uses Let's Encrypt rate-limit quota.
13. `doctor` exits 1: warning `the decoder image is 20777 days old` (the host decoder reports no build date); the worker reports slots ready.
