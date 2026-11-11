# sunnycloud — Environment Strategy

**Status**: draft (planning, no code yet)
**Date**: 2026-11-04
**Sources**: `architecture.md` §3 (environments, per-environment configuration); `restrictions.md`; `tests/environment.md`; AC-22, AC-30, AC-41, AC-43, AC-63.

Setting names use the prefix `SUNNYCLOUD_`; the key-to-variable rule is in `common-helpers/01_helper_config.md` and each component spec lists its keys. The shape is not provisional: typed config, validated at start, every secret readable from a file, every duration configurable.

## Environments

| Environment | Where | How it is built and run | Data | Purpose |
|-------------|-------|-------------------------|------|---------|
| Development | Developer workstation | `compose.dev.yml` or host `cargo run` against the compose database; `-test`-feature API; mock-idp | Throw-away, seeded by tests | Daily work |
| Test (CI) | GitHub-hosted runner; self-hosted tiers for performance and large files | `tests/compose.test.yml`, SUT images by digest, short durations | Golden snapshot after migration and admin bootstrap, reset in under 60 s | Gate for every PR and push |
| Staging | The `stage` branch: CI-built, signed images tagged `stage-<sha>` | Same `core` stack plus nightly suites. **No shared staging host exists yet**; if the owner provisions one it runs the release compose with the stage images | Synthetic | Pre-release evidence; fast-forward from `dev` at loop close |
| Production | The operator's own host (Docker Compose, release bundle) | `deploy/compose.yml` with release image digests | Operator data | The product |
| Demo (optional) | Hosting not decided | Same as production | Synthetic | Only if the owner chooses to run one (owner decision) |

The project itself operates no production service. "Production" means the documented operator install; its quality is judged by AC-22 and AC-65.

## Configuration

Plain configuration comes from environment variables (a `.env` file next to `compose.yml`); anything secret comes from files under `/run/secrets` through `<NAME>_FILE`. The server and worker bind a typed config at start and refuse to start on a missing required value, a malformed duration, an unknown `SUNNYCLOUD_*` key, a master key whose length is not 32 bytes, or a bound outside its documented range. No raw key reads in business code.

### Required for an operator install

| Variable | Example / default | Purpose |
|----------|-------------------|---------|
| `PUBLIC_HOST` | `cloud.example.org` | Caddy site name and TLS name |
| `SUNNYCLOUD_PUBLIC_URL` | `https://cloud.example.org` | Redirect URIs, links, cookie scope; must be `https` outside dev |
| `SUNNYCLOUD_VERSION` | `1.0.0` | Image tag for api, worker, decoder |
| `BLOB_PATH` | `/srv/sunnycloud/blobs` | Host path of the XFS (or ext4) blob volume |
| `SCRATCH_PATH` | `/srv/sunnycloud/scratch` | Host path of the `decoder-general` scratch: a dedicated mount or a directory under an XFS project quota (hard size cap) |
| `SUNNYCLOUD_TRUSTED_PROXIES` | Caddy's fixed address on `proxy` (`172.29.1.2`) | Default deny: unset means `X-Forwarded-For` is ignored and every client has the proxy's address (AC-20 would then rate-limit everyone as one) |
| `SUNNYCLOUD_BLOB_FREE_RESERVE` | `20GiB` | Free-space floor; uploads and sessions are refused below it (AC-39) |
| `SUNNYCLOUD_DEFAULT_QUOTA` | `100GiB` | Per-user quota default; admin override per user |
| `SUNNYCLOUD_GITHUB_CLIENT_ID` and/or `SUNNYCLOUD_OIDC_ISSUER`, `SUNNYCLOUD_OIDC_CLIENT_ID` | | At least one provider (AC-19); client secrets are files |
| `SUNNYCLOUD_ALLOWLIST_EMAILS` | empty | Optional verified emails allowed to redeem an invite |

Optional: `WORKER_CPUS` (default 2; at most half the host cores) and `WORKER_MEM` (default 4g) for the `decoder-general` container (`decoder-interactive` is fixed at 2 CPU and 2 GiB), `SUNNYCLOUD_INTERNAL_ADDR` (the worker listener; the install sets it to the API's `backend` address, `172.29.0.2:8081`, so the listener is not reachable from `proxy`; the key's default `0.0.0.0:8081` is for dev), `SUNNYCLOUD_SHARE_MAX_TTL` (default `365d`), `SUNNYCLOUD_NVENC` (v1.1, default `off`), `SUNNYCLOUD_LOG_LEVEL` (default `info`), `SUNNYCLOUD_AUDIT_RETENTION` (default `365d`, `0` = keep), `SUNNYCLOUD_SCRUB_INTERVAL`, `SUNNYCLOUD_SCRUB_MAX_RATE`.

### Durations: production default versus test value

Every duration is configuration (architecture principle). Values accept `humantime` strings (`7d`, `10m`, `20s`). Tests use short values so a run does not take days; the injectable clock (`POST /test/clock` on `:9091`, `-test` images only; the supervisor follows it through `GET /internal/v1/test/clock-offset`) is used only where the AC asserts the production default itself (group `X` in `tests/environment.md`).

| Setting | Production default | Range | Test stack value | Source |
|---------|--------------------|-------|------------------|--------|
| `SUNNYCLOUD_RETENTION` (replaced and deleted blobs) | `7d` | `0` to `90d` | `30s` | AC-30 |
| `SUNNYCLOUD_TOMBSTONE_HORIZON` | `90d` | fixed minimum 90d in release builds | `60s` | AC-41 |
| `SUNNYCLOUD_DEVICE_CODE_TTL` | `10m` | max `10m` | `20s` | AC-57 |
| `SUNNYCLOUD_JOB_LEASE` | `5m` | | `10s` | restrictions |
| `SUNNYCLOUD_JOB_ATTEMPTS` | `3` | | `3` | restrictions |
| `SUNNYCLOUD_UPLOAD_SESSION_TTL` | `24h` | | `60s` | restrictions |
| `SUNNYCLOUD_ORPHAN_SWEEP_AGE` | `48h` | | `120s` | restrictions |
| `SUNNYCLOUD_SHARE_DEFAULT_TTL` | `7d` | | `30s` | AC-12 |
| `SUNNYCLOUD_SHARE_MAX_TTL` | `365d` | | `60s` | decision 15 |
| `SUNNYCLOUD_REFRESH_REUSE_GRACE` | `30s` | | `5s` | AC-58 |
| `SUNNYCLOUD_PROVIDER_TIMEOUT` | `10s` | | `2s` (mock slow provider answers after it) | restrictions |
| `SUNNYCLOUD_DECODER_STALL_KILL` | `90s` | | `5s` | AC-45 |
| `SUNNYCLOUD_PHOTO_JOB_TIMEOUT` / `..._VIDEO_JOB_TIMEOUT` | `120s` / `40m` | | `10s` / `60s` | AC-25 |
| `SUNNYCLOUD_SCRUB_INTERVAL` | `7d` | | `10s` (manual trigger through `e2e-ctl`) | AC-24 |
| `SUNNYCLOUD_DB_RECOVERY_BUDGET` | `60s` | | `60s` (asserted, not shortened) | restrictions |

Rules: release builds enforce the ranges (retention 0 to 90 d, code TTL at most 10 min, tombstone horizon at least 90 d); only the `test-clock` build relaxes the lower bounds so tests can use seconds. `RETENTION=0` is valid in release builds (no safety net, sensible only with volume snapshots) and `doctor` warns about it. Client-side durations (2 s settle, mass-delete thresholds) belong to the clients' own config.

### `.env.example` (shipped in the release bundle)

```dotenv
# Copy to .env and fill in. Never commit .env or secrets/.
PUBLIC_HOST=cloud.example.org
SUNNYCLOUD_PUBLIC_URL=https://cloud.example.org
SUNNYCLOUD_VERSION=1.0.0
BLOB_PATH=/srv/sunnycloud/blobs
SUNNYCLOUD_TRUSTED_PROXIES=172.29.1.2
SUNNYCLOUD_INTERNAL_ADDR=172.29.0.2:8081
SCRATCH_PATH=/srv/sunnycloud/scratch
SUNNYCLOUD_BLOB_FREE_RESERVE=20GiB
SUNNYCLOUD_DEFAULT_QUOTA=100GiB
SUNNYCLOUD_GITHUB_CLIENT_ID=
SUNNYCLOUD_OIDC_ISSUER=
SUNNYCLOUD_OIDC_CLIENT_ID=
WORKER_CPUS=2
WORKER_MEM=4g
```

## Secrets

| Secret | Created by | Stored | Read by | Rotation |
|--------|-----------|--------|---------|----------|
| Master key (32 bytes) | Operator, once, at install | `secrets/master_key` mode 0400, **and** the backup copies below | api only | `sunnycloud admin rewrap` (AC-43); old key kept while any backup still uses it |
| Postgres password, `db_api_url`, `db_migrator_url` | Operator at install | `secrets/` | db (password); api (`db_api_url`); the `migrate` service (`db_migrator_url`). The worker reads none | Change and restart; no application impact |
| `worker_api_token` | Operator at install | `secrets/` | api, worker | Two-secret rotation without a refusal window: `deployment_procedures.md` section "Worker secret rotation" (`worker_api_token_next` on the API first) |
| `share_grant_key` | Operator at install (`openssl rand -out secrets/share_grant_key 32`) | `secrets/` | api only | Change and restart; outstanding share access grants stop working, links and passwords are unaffected |
| Provider client secrets | Provider console | `secrets/` | api | Per provider policy |
| Apple `.p8` (v1.1) | Apple developer console | `secrets/` as a file | api | Apple key lifetime; the server regenerates the ES256 client secret itself, so no six-month manual step |
| Setup token | Operator (optional) or the server at first start | `secrets/setup_token` or printed once | api until the first admin exists | Voided at use (AC-23) |
| Updater keys (active and recovery Ed25519) | Owner, offline | Never in CI: the manifest is signed offline or in a manual job; the recovery key stays in cold storage | the owner only | Active key rotated through a manifest signed by the recovery key; see `ci_cd_pipeline.md` |
| CI: image signing (keyless OIDC), store keys | Owner | GitHub Environment `release`; the Woodpecker fallback holds no signing secret | `release` jobs only | Per key; see `ci_cd_pipeline.md` |

Handling rules: secrets never enter git, images, env files, logs or metrics labels (`.gitignore` covers `secrets/`, `.env`, `e2e-results/`); the install guide shows file creation with `umask 077`; a repository secret scan (gitleaks, SHA-pinned) runs in the PR gate. The setup token is the one value the server may print, once, clearly marked, only when it generated the token itself.

### Master-key backup rule (AC-43, AC-65)

1. Loss of the master key makes all data unrecoverable. The install guide says so before the key is created.
2. The key is stored **separately from the data backups**: never in the same archive, bucket or repository as the database dump or the blobs. A stolen backup must not contain its own key.
3. At least **two independent copies** in two places (for example a password manager entry and an offline printed or USB copy).
4. A **restore drill** proves the key: the restore procedure fails closed if the key does not match the installation's key-check value, and `sunnycloud doctor` verifies the mounted key against that value.
5. After a rotation, keep every key that a retained backup still depends on. A dump from before the rotation holds blob keys wrapped under the old key.
6. The key never goes into the CI test stack except as a throw-away generated per run.

The server refuses to start with a key that does not match the installation: a key-check value is stored at first start (`.keycheck` on the blob volume, owned by 04). For a restored volume the operator confirms the key and runs `sunnycloud admin keycheck-init`.

## Database management

| Topic | Rule |
|-------|------|
| Engine | PostgreSQL 18 everywhere (dev, CI, prod); CI has a dedicated PG 18 job (`pg18-migrations`) |
| Roles | `sunnycloud_migrator` (owns the schema; bypasses RLS as owner; used only by the one-shot `migrate` service) and `sunnycloud` (API runtime: DML only, no DDL, no `BYPASSRLS`, RLS policies enabled; `audit_log` INSERT and SELECT only, retention purge is a `SECURITY DEFINER` function owned by the migrator); created by the init script on an empty data directory, later privileges by migrations. The worker and the decoders have no database role. Backups run as the Postgres superuser inside the db container. Details in `common-helpers/07_helper_db.md` |
| Migrations | sqlx, run only by the one-shot `migrate` Compose service (`sunnycloud migrate`, role `sunnycloud_migrator`) before the API starts, under an advisory lock, each in a transaction. The API does not migrate: at start it checks the schema version and refuses to serve on a mismatch, so an interrupted migration leaves the API down until `migrate` is re-run. `down` files exist for CI and `migrate revert`; the operator's rollback after a committed migration is a restore (see `deployment_procedures.md`) |
| Seed data | None in production. Test stack: migrate, bootstrap admin through the setup token, snapshot |
| Minor updates | Pull the `postgres:18` digest shipped with the release |
| Major upgrade | Dump and restore into a new volume (documented per major); never automatic; no auto-update tools |
| Time | All product timestamps come from the injectable clock, not SQL `now()`, so retention and expiry tests can move time |
| Filesystem | Data on local SSD; the blob volume on XFS (documented default) or ext4; ZFS/btrfs mirror and ECC RAM recommended; volume encryption recommended on top of application encryption |

## Per-environment differences

| Config | Development | Test (CI) | Staging | Production |
|--------|-------------|-----------|---------|------------|
| Database | Compose PG 18 on localhost:5432 | Compose PG 18, backend-net only | Compose PG 18 | Compose PG 18, operator volume and backups |
| TLS | Caddy `tls internal` on `sunnycloud.localhost` | Test CA, `sunnycloud.test` | ACME or test CA | ACME (public DNS, ports 80/443) or the operator's proxy |
| Providers | mock-idp | mock-idp; live accounts only for `H-LIVE` | mock-idp | Operator's registered apps |
| Secrets | `secrets-dev/` throw-away | Generated per run | CI-generated | Operator files, 0400 |
| Durations | Short | Short, clock for group `X` | Short | Defaults above |
| Logging | Console JSON | JSON to report artefacts | JSON | JSON to the operator's collector |
| Images | Built locally | SUT by digest, `-test` API | `stage-<sha>` signed | Release by version, signed, digests in `images.lock` |
| Clock endpoints | Present (`test-clock`) | Present in the `-test` images only | Absent | Absent (CI-asserted) |

## AC coverage

| AC | Element |
|----|---------|
| AC-22 | Short required-variable list, `.env.example`, `doctor` validation, file secrets |
| AC-30 | `SUNNYCLOUD_RETENTION` range and test clock |
| AC-41 | Tombstone horizon, epoch bump on restore (see procedures) |
| AC-43 | Master-key rule, file secret, api-only mount, `keycheck-init`, `rewrap` |
| AC-63 | Pinned images, secret scanning, signing keys in a protected Environment |
| AC-64 | `SUNNYCLOUD_MIN_CLIENT_VERSION` is set by the release, overridable by the operator |
