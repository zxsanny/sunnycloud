# MVP stack runbook (zxstation, `https://drive.zxsanny.com`)

Postgres 18 and Caddy run in Docker (`deploy/compose/compose.mvp.yml`, `deploy/compose/Caddyfile.mvp`). The API, the worker and two decoder-runners (interactive and general lane) run as host processes from release binaries. Nothing here is a release bundle: it is the MVP host setup of `_docs/02_document/mvp_roadmap.md` (package A). Exposure facts: `mvp_exposure_review.md`.

## Files

| What | Where |
|---|---|
| Scripts | `scripts/dev/mvp-secrets.sh`, `mvp-up.sh`, `mvp-down.sh`, `mvp-status.sh`, `mvp-cli.sh` (shared code in `mvp-lib.sh`) |
| Secrets and the editable settings (gitignored) | `secrets-dev/` |
| Settings example (committed) | `deploy/mvp/mvp.env.example`; the live copy is `secrets-dev/mvp.env` |
| Data, pid files, logs | `/develop/zxsanny/sunnycloud-data/{blobs,scratch-*,run}` (`run/<process>.log`, `run/<process>.pid`, `run/build.log`) |
| Binaries | `/develop/zxsanny/targets/stack/release/` (`CARGO_TARGET_DIR`; `src/target` is not touched) |
| Web bundle | `src/web/dist` when it has an `index.html`, else `deploy/mvp/web-placeholder` |

## Start, check, stop

```bash
scripts/dev/mvp-up.sh       # secrets, build, docker (db + caddy), migrate, key check, 4 host processes, health waits
scripts/dev/mvp-status.sh   # docker state, the four pids, the three health URLs, free disk; exit 1 when anything is down
scripts/dev/mvp-down.sh     # SIGTERM to worker, api, decoders (60 s grace each), then docker compose down
```

- `mvp-up.sh` is safe to run twice: a running process is left alone, `migrate` and `admin keycheck-init` are idempotent, Caddy is recreated only when its compose config changed. A new binary does not replace a running process: run `mvp-down.sh` then `mvp-up.sh`.
- A cold build takes minutes; the script prints progress every 30 s and the full output is in `run/build.log`.
- `mvp-down.sh` keeps the Docker volumes (database `pgdata`, certificates `caddy_data`) and everything under `sunnycloud-data`. Never run `docker compose down -v`: it deletes the database and the certificates.
- Start refuses when less than 50 GiB is free (`MVP_MIN_FREE_GIB`).
- The three health URLs: `http://127.0.0.1:9090/readyz` (API ops), `http://127.0.0.1:9092/readyz` (worker ops), `https://drive.zxsanny.com/api/v1/server-info` (through Caddy).

## Logs

```bash
tail -f /develop/zxsanny/sunnycloud-data/run/api.log            # also worker.log, decoder-interactive.log, decoder-general.log
docker logs -f sunnycloud-mvp-caddy-1                            # ACME, TLS and access errors
docker logs -f sunnycloud-mvp-db-1
```

## CLI and health report

```bash
scripts/dev/mvp-cli.sh doctor                 # exit 1 on a warning
scripts/dev/mvp-cli.sh admin setup-token      # prints the one-time token while no account exists
scripts/dev/mvp-cli.sh admin invite --admin <admin account id> create   # see `mvp-cli.sh admin invite --help`
```

## Certificate: staging now, production later

The default CA is Let's Encrypt **staging**: the certificate is real but untrusted by browsers (use `curl -k` or `--cacert`). To switch:

1. In `secrets-dev/mvp.env` uncomment `ACME_CA=https://acme-v02.api.letsencrypt.org/directory`.
2. `scripts/dev/mvp-up.sh` (Caddy is recreated with the new variable and obtains a production certificate; storage is keyed by CA, so the staging certificate stays unused in `caddy_data`).
3. Check: `echo | openssl s_client -connect 127.0.0.1:443 -servername drive.zxsanny.com 2>/dev/null | openssl x509 -noout -issuer` must not say `(STAGING)`.

Optional: put a contact address into `secrets-dev/acme_email` (a line starting with `PLACEHOLDER` means "no email"). Do this before the production switch if expiry notices are wanted; it changes the ACME account, which is harmless.

Requirements: ports 80 and 443 reachable from the internet (HTTP-01 on 80). With the Cloudflare proxy on, the challenge arrives through Cloudflare and works (shown on the first staging issue); Cloudflare SSL mode must be "Full" or "Full (strict)", and "Full (strict)" works only with the production certificate.

## Google sign-in

The provider is the generic OIDC one with issuer `https://accounts.google.com`. Create the client as in `mvp_roadmap.md` section 8; the redirect URI is `https://drive.zxsanny.com/api/v1/auth/providers/oidc/callback` (printed by `doctor`). Then:

```bash
printf '%s' '<client id>'     > secrets-dev/oidc_client_id
printf '%s' '<client secret>' > secrets-dev/provider_oidc_client_secret
scripts/dev/mvp-down.sh && scripts/dev/mvp-up.sh      # the API reads both at start
```

`mvp-up.sh` warns while either file still starts with `PLACEHOLDER`.

## First account (setup)

Before running it, bind the admin to your Google address: uncomment and edit `SUNNYCLOUD_ALLOWLIST_EMAILS=` in `secrets-dev/mvp.env`, restart. Then:

1. `scripts/dev/mvp-cli.sh admin setup-token` and keep the printed token secret (valid 7 days, one open at a time; running the command again replaces it; it stops working once the first account exists).
2. The sign-in begins with `POST /api/v1/auth/login-attempts` carrying `{"provider":"oidc","return_to":"/","client_kind":"web","setup_token":"<token>"}` (the web setup page is to do this); Google asks for consent and returns to the callback, which creates the first admin.
3. `GET /api/v1/auth/providers` then reports `"setup_required": false`.

After that, accounts need an invite: `scripts/dev/mvp-cli.sh admin invite --admin <admin account id> create`.

## Secrets

`scripts/dev/mvp-secrets.sh` creates `secrets-dev/` (mode 700; files 600, except `postgres_password`, `db_api_url` and `db_migrator_url` at 644 because the Postgres container reads them as uid 999) and never overwrites a file. It never prints values. **Back up `secrets-dev/master_key` somewhere outside this disk**: without it every stored file is unreadable (`environment_strategy.md`, master-key backup rule).
