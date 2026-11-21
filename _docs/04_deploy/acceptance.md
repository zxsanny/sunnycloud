# Acceptance stack: the whole product path without a real Google sign-in

A second, isolated copy of the MVP stack plus a mock OpenID Connect provider, a one-origin web server and a Playwright
smoke test. It proves the path browser, SPA, API, worker, decoder and Postgres end to end. It never touches the live MVP
stack (`mvp_runbook.md`).

## Run

```bash
export PATH="$HOME/.bun/bin:$PATH"                          # Bun 1.4.5
scripts/dev/acceptance-up.sh                                # build (own target), database, mock IdP, api, worker, 2 decoders, web server
scripts/dev/acceptance-status.sh                            # units, container, five health URLs, listeners; exit 1 when anything is down
scripts/dev/acceptance-curl-check.sh                        # sign-in proof without a browser: setup token, then an invite for bob, then the WebSocket upgrade
scripts/dev/acceptance-down.sh && scripts/dev/acceptance-up.sh   # the setup token works once per database, so the next run starts fresh
(cd tests/e2e && bun install && bun run test)               # Playwright smoke (E2E_SHOTS=<dir> keeps the screenshots)
scripts/dev/acceptance-down.sh                              # removes everything below
scripts/dev/acceptance-cli.sh admin setup-token             # the server CLI against this stack
```

`ACC_SKIP_BUILD=1` reuses the binaries and the web bundle of the last start. A cold build is about a minute; progress is printed every 30 s.
Playwright is pinned at 1.65.0, whose Chromium revision 1254 is the one in `~/.cache/ms-playwright`; on another host run `bunx playwright install chromium` once.

## What is separate from the live stack

| | Live MVP | Acceptance |
|---|---|---|
| Origin | `https://drive.zxsanny.com` (Caddy 80/443) | `http://localhost:18800` (Bun web server) |
| Loopback ports | 8080, 8081, 9090, 9092, 5532 | 18800 to 18806 (`ACC_PORT_BASE`; start refuses a taken port) |
| systemd --user units | `sunnycloud-mvp-*` | `sunnycloud-acc-{api,worker,decoder-interactive,decoder-general,idp,web}` |
| Database | Postgres container, volume `pgdata`, database `sunnycloud` | container `sunnycloud-acc-db`, database `acceptance`, data on tmpfs |
| Data, secrets, logs | `sunnycloud-data/`, `secrets-dev/` | `/develop/zxsanny/sunnycloud-acceptance/` (`ACC_DATA`, must end in `sunnycloud-acceptance`) |
| Binaries | `/develop/zxsanny/targets/stack` | `/develop/zxsanny/targets/acceptance` (cargo target and a copy of `src/web` with its bundle; a build cache that `down` keeps, `ACC_PURGE_BUILD=1` removes) |
| Cookies | host `drive.zxsanny.com` | host `localhost` (cookies ignore ports, but a fresh Playwright context has none) |

Everything listens on 127.0.0.1 only (the mock IdP has no setting to change that). `acceptance-down.sh` leaves no unit,
container, database or data directory. The scripts source `mvp-lib.sh` and override only the unit name, the data directory and the target.

Why a throwaway database container and not a second database in the MVP Postgres: roles are cluster-wide (the schema
owner and revokes of `10-roles.sh` would be shared with the live database), a test run would compete with the live
server for connections and disk, and dropping it must leave nothing behind. The container uses the MVP's pinned image and the same `10-roles.sh`.

## Why `development`

The server refuses plain http in three places, so the acceptance settings are `environment=development` and `allow_plain_http_dev=true`:

1. `shared/config/src/section.rs` `check_public_url`: an `http://` `public_url` is `ForbiddenInRelease` unless `environment=development`.
2. `components/01_identity/src/config.rs` `check_public_url`: an `http://` `public_url` also needs `allow_plain_http_dev`.
3. `components/01_identity/src/outbound/destination.rs` `url_refused`: the provider client refuses an `http` issuer unless `allow_plain_http_dev`. Loopback is not refused (only link-local and metadata addresses are).

`development` also lifts the lower bound of duration settings; the stack sets no short duration. Cookies stay `__Host-` and `Secure`: Chromium
treats `http://localhost` as secure, and the test asserts that the session cookie is kept (`secure`, `HttpOnly`, `SameSite=Lax`).

## Pieces

- `tests/mocks/idp/`: the mock IdP (Bun, no framework, no runtime dependency). Discovery, `/authorize` (auto-approves the configured user, PKCE S256 and `state` required, exact redirect URIs), `/token` (RS256 id token with `iss aud sub email email_verified name nonce iat exp`, key generated at start, one-use codes), `/jwks`. Users `alice` (the default) and `bob`; `POST /__control/user {"user":"bob"}` switches the default. `cd tests/mocks/idp && bun test`.
- `tests/e2e/server/web-server.ts`: one origin for the SPA and the API (standing in for Caddy): the SPA with the CSP of `Caddyfile.mvp`, `/api/*` proxied with streaming and the original `Host`, and the WebSocket upgrade of `/api/v1/realtime`. A refused upstream handshake is a 502 here, not the API's status.
- `tests/e2e/tests/smoke.spec.ts`: the Playwright test (serial). Media files are generated at test time (JPEG and HEIC of 3000 px, PNG, WebP, MP4, MOV, a 40 MiB random file, a text file with a markup file name).

## What the smoke test proves

First-run setup page with the setup token, sign-in through the mock IdP, a folder, uploads through the real file input (small and large paths), then after a reload: every image and video row loads a real thumbnail (at most 320 px wide), no `<video>` element exists, the lightbox opens a 1440 px WebP preview of an image and the poster (`size=small`) of a video, downloads of three files match their source SHA-256, a markup file name is text, and the whole run has no CSP violation, page error or console error. Thumbnails come from the real worker and decoder; the test reloads (bounded, 150 s) because the web client has no realtime push yet.

## What it does not prove

- Real Google (consent screen, `accounts.google.com` discovery, key rotation, the client's redirect URI registration).
- Cloudflare, Let's Encrypt and Caddy (TLS, the h2c upstream, HTTP-01, the real WebSocket path through the proxy); this stack uses a Bun proxy over http.
- Safari and iOS (and Firefox): Chromium only.
- Realtime in the browser (the web client for it does not exist yet; the curl check proves only the upgrade and the first frame).
- Production-mode validation (`environment=production`, https `public_url`), the container decoder stage, a 1 GB video, the transcode path (its jobs end `dead` with `Unsupported`), more than two sessions at once.

## Edge cases and concurrency (also in the smoke test)

- A 0-byte file, a 255-character name, a Ukrainian and emoji name, a text file named `.jpg` and two files with the same name all upload. The empty, long, Unicode and fake-JPEG files end as rows with a "No preview" icon; the Unicode file downloads under its own name with the same bytes.
- Two files with the same name in one folder are both kept: the second is listed as `same-name (conflict copy <date time> web).txt`.
- Two browser contexts of the same user upload a fresh 40 MiB file at once; both transfers finish, both files are listed and a download matches the source hash.
- The page ignores a file pick made before the folder has loaded (no message). The test waits for the loaded folder first.
