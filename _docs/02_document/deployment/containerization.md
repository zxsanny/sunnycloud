# sunnycloud — Containerization

**Status**: draft (planning, no code yet)
**Date**: 2026-11-04
**Sources**: `architecture.md` §3, §5, §7; `restrictions.md`; `acceptance_criteria.md` (AC-22, AC-24, AC-25, AC-43, AC-63); `tests/environment.md` (test stack); `00_research/raw/b_ops_licence_scope.md` (F5, F12, F13, F20).

Image and file names below are the plan; paths such as `deploy/docker/` are proposed and become real in the implement step. Digests are written `<pin>` until the first build records them.

## Images

Three images (plus the `-test` tags of two) are built from the monorepo and two third-party images are pinned. Every base image is referenced by digest (RESTRICT-30).

| Image | Built from | Purpose | Runs as | Approx. target size |
|-------|-----------|---------|---------|--------------------|
| `ghcr.io/zxsanny/sunnycloud-api` | `deploy/docker/api.Dockerfile` | The one `sunnycloud` binary: `serve` (API, janitor loops and scheduler), `migrate` (run by the one-shot `migrate` Compose service, not by `serve`), `doctor`, `scrub`, `admin`, `healthcheck`, `openapi`, `version`; embedded web bundle | uid 10001 | < 100 MB |
| `ghcr.io/zxsanny/sunnycloud-worker` | `deploy/docker/worker.Dockerfile` | Trusted supervisor only (Rust binary, no media libraries); talks only to the API on `:8081` | uid 10002 | < 50 MB |
| `ghcr.io/zxsanny/sunnycloud-decoder` | `deploy/docker/decoder.Dockerfile` | Untrusted decoder: `decoder-runner`, ffmpeg, libvips, libheif; run as two containers per supervisor | uid 10003 | < 700 MB |
| `ghcr.io/zxsanny/sunnycloud-api:<tag>-test` and `ghcr.io/zxsanny/sunnycloud-worker:<tag>-test` | the same Dockerfiles with `FEATURES=test-clock` | Test images: the API adds `POST /test/clock` on `:9091` and `GET /internal/v1/test/clock-offset` on `:8081`; the supervisor polls the latter | uid 10001 / 10002 | same as the release image |
| `caddy:2@sha256:<pin>` | upstream | TLS and routing | root in container, `cap_drop: ALL` + `NET_BIND_SERVICE` | |
| `postgres:18-trixie@sha256:<pin>` | upstream | Database | postgres (uid 999) | |

Why two worker images instead of one: the supervisor holds the worker bearer token and must not ship ffmpeg, libvips or libheif (they are the CVE-prone part, ADR-004); it has no database login at all (it claims and reports jobs through the API on `:8081`); the decoder holds no credentials and is the only image that carries GPL code (ADR-010), so the source-offer duty and the weekly rebuild focus on it. Cost: one extra Dockerfile that reuses the same Rust builder stage.

Janitor loops (session reaper, GC, orphan sweep, scrub step, tombstone purge and the others) run inside `sunnycloud serve`, not in extra containers (decision 16). v1.0 runs exactly one API process: upload progress is held in memory, so replicas are unsupported for uploads (decision 12) and the compose file sets no `replicas`.

The Svelte web bundle is built in the api Dockerfile and served by the API (one artefact, always in lockstep with the API version). If the plan chooses a separate static container, only this section and the Caddyfile change.

### API server (`api.Dockerfile`)

| Property | Value |
|----------|-------|
| Build image | `rust:<stable>-trixie@sha256:<pin>` (toolchain pinned in `rust-toolchain.toml`; MSRV 1.88 from notify 8.2) |
| Web stage | `node:<lts>-trixie-slim@sha256:<pin>`: `npm ci`, `npm run build` |
| Stages | web build → cargo-chef planner → dependency cook (cached layer) → `cargo build --release --locked -p sunnycloud-server --features ${FEATURES:-}` with `SQLX_OFFLINE=true` → runtime |
| Runtime base | `gcr.io/distroless/cc-debian13:nonroot@sha256:<pin>` (rustls, no OpenSSL, no shell) |
| Contents | `/usr/local/bin/sunnycloud`, `/usr/share/sunnycloud/web/`, `/usr/share/sunnycloud/openapi.json`, migrations embedded in the binary |
| User | `10001:10001`, no shell, read-only root filesystem at run time |
| Entrypoint | `["sunnycloud"]`, default `CMD ["serve"]` |
| Ports | 8080 public listener (`proxy` network; Caddy reaches it over h2c), 8081 internal listener (`/api/v1/worker-jobs/**`, worker bearer) and 9090 ops (`/healthz`, `/readyz`, `/metrics`): both bound to the API's fixed address on the `backend` network only (`SUNNYCLOUD_INTERNAL_ADDR=172.29.0.2:8081`; the ops address follows the same rule, its key is still to be added to helper 01), never routed by Caddy; 9091 only in the `-test` image (clock control) |
| Database role | `sunnycloud` (DML only: no DDL, no `BYPASSRLS`; `audit_log` INSERT and SELECT only). The API never migrates: at start it compares the schema version with its own and refuses to serve on a mismatch; the schema is changed only by the `migrate` service |
| Health check | `["CMD","sunnycloud","healthcheck"]` calls the ops address `/readyz` (no curl in the image); `start_period: 120s` |
| Build args | `FEATURES` (empty for release, `test-clock` for the test image), `VERSION`, `REVISION` |
| Labels | OCI source/version/revision/licenses (`AGPL-3.0-only`), `org.sunnycloud.test-clock=true|false`; decoder: `org.sunnycloud.variant=test-stubs` on the test-stubs image only |
| Stop | `SIGTERM`, `stop_grace_period: 120s` (in-flight blocks finish; interrupted uploads resume from the received-bitmap, AC-3) |

Release-versus-test rule: the release image is compiled without the `test-clock` feature, so the routes do not exist. CI asserts, in `images` and again in `release.yml` against the **pushed `@digest` of the api and the worker image before signing**, that `POST /test/clock` and `GET /internal/v1/test/clock-offset` return 404 and that the label is `false`; the `-test` tag is never promoted to a release tag or signed as a release (AC-30).

### Worker supervisor (`worker.Dockerfile`)

| Property | Value |
|----------|-------|
| Build image | same Rust builder as the API (shared cargo-chef layers) |
| Runtime base | `gcr.io/distroless/cc-debian13:nonroot@sha256:<pin>` |
| Contents | `/usr/local/bin/sunnycloud-worker` only |
| User | `10002:10002` |
| Secrets | `worker_api_token` (bearer for the internal listener `:8081`). **No** database URL or role (there is no worker database login), **no** `master_key`, **no** blob mount, no provider secrets |
| Networks | `backend`; it reaches only the API on `:8081` (claim, heartbeat, input, output, fail), never Postgres; no network shared with the decoders |
| Memory | `mem_limit: 512m` (default; the supervisor streams input to scratch and holds no media in memory) |
| Decoder access | Two Unix sockets, one per decoder container, each in its own shared socket volume (`sock_interactive`, `sock_general`), and the scratch volumes below; no Docker socket |
| Ports | 9090 ops (`/healthz`, `/readyz`, `/metrics`), backend-net only |
| Health check | `["CMD","sunnycloud-worker","healthcheck"]`; `/readyz` includes "both decoders reachable" |
| Start-up self-check | Before taking jobs the supervisor asks each decoder to prove isolation (no network interface but loopback; no env secrets; no blob mount). Failure: refuse to start, log, `sunnycloud_worker_selfcheck_ok 0` |

### Decoder (`decoder.Dockerfile`) — AC-25, AC-63

One image, two containers per supervisor (decision 13). `decoder-interactive` serves only the thumbnail lane (the reserved slot for AC-10) with tmpfs scratch of a fixed numeric size; `decoder-general` serves thumbnails and transcodes with disk-backed scratch on a dedicated mount (or an XFS project quota) at `${SCRATCH_PATH}`, because a multi-gigabyte video in RAM would count against its memory limit and an unbounded scratch directory could fill the host disk. Each container runs one job at a time.

| Property | Value |
|----------|-------|
| Base | `debian:trixie-slim@sha256:<pin>` (runtime and builder share the Debian release so Debian libraries link cleanly) |
| Stages | `fetch` (tarballs from `versions.env`, SHA-256 verified) → `build-media` (libheif, libvips, ffmpeg into `/opt/media`) → `build-runner` (Rust `decoder-runner`) → runtime (copy `/opt/media`, install runtime libraries only) |
| Source-built (Debian cannot give the needed version or flags) | **libheif >= 1.22.0**, **libvips 8.18.7**, **ffmpeg** (pinned tag) |
| From Debian packages (patched by `apt-get dist-upgrade` on every rebuild) | libx264, libde265, dav1d, libwebp, libjpeg-turbo, libpng, libexif, libzimg, lcms2 |
| User | `10003:10003`, `read_only: true`, scratch volume (below), `cap_drop: ALL`, `no-new-privileges` |
| Environment | `VIPS_BLOCK_UNTRUSTED=1`, `VIPS_CONCURRENCY` set per job by the runner; no credentials of any kind |
| Entrypoint | `["decoder-runner"]` (listens on its Unix socket for one job at a time from its supervisor; probes only, the transcode policy is applied by the supervisor; wraps each `vipsthumbnail` or `ffmpeg` call with CPU/memory/wall-time limits and the protocol/format whitelists; separate HEIC profile) |
| Network | `network_mode: none`; the socket volume is the only channel |
| Health check | `["CMD","decoder-runner","health"]` |

**ffmpeg build (licence and attack surface)**: configured with `--enable-gpl --enable-libx264 --enable-libzimg` (HDR tone mapping), the native `aac` encoder, and **without** `--enable-nonfree`, `libfdk_aac`, `libx265`, CUDA/NPP filters. Attack-surface trimming at compile time: `--disable-autodetect --disable-network --disable-devices --disable-doc --disable-ffplay`, protocols limited to `file` and `pipe`, demuxers limited to an allowlist of what phones and cameras produce (mov/mp4, matroska/webm, avi, mpegts, a few raw bitstreams), no `hls`, `dash`, `concat`, `tee`, `data` demuxers (an HLS playlist or concat file naming `/etc/passwd` has nothing to parse it, AC-25). The exact lists are fixed by gates S5 and S10 and the red-team corpus; the build recipe lives in `deploy/docker/ffmpeg.configure`.

**libvips build**: loader allowlist (jpeg, png, webp, heif for HEIC/AVIF, gif, plus whatever S4 shows phones need). SVG, PDF, ImageMagick, OpenSlide, FITS, NIfTI, Matlab, OpenEXR loaders are compiled out. libheif is decode-only (`WITH_X265=OFF`, no AOM/rav1e/SVT encoders; libde265 and dav1d for decode); HEVC patent note goes in the README.

**Build checks that run in CI on every build (`scripts/ci/check-decoder-image.sh`)**:
- `ffmpeg -version` contains `--enable-gpl`, does not contain `--enable-nonfree`; `ffmpeg -encoders` lists `aac` and `libx264` and lacks `libfdk_aac` and `libx265`.
- `ffmpeg -protocols` lists only `file` and `pipe`; `ffmpeg -demuxers` lacks `hls`, `concat`.
- `pkg-config --modversion libheif` >= 1.22.0; `vips --version` is 8.18.7; `vips -l` lacks svg, pdf, magick loaders.
- The release decoder image has no test hooks (AZ-3850 and AZ-3851, 10 description sections 13.4 and 13.6), checked on the built image and again on the pushed `@digest` before signing: `docker export | tar -t` lists no `decoder-test-stub` and no `*.real`; `sha256sum` of `ffmpeg`, `ffprobe`, `vipsthumbnail` and `vipsheader` equals the build-stage manifest; `strings` of `decoder-runner` finds no `SCSTUB1` and no `test_freeze_main_loop`, and `nm` finds no `test_freeze` symbol; the image has no label `org.sunnycloud.variant=test-stubs` (that label is set only on the `<tag>-test` stub image, which the harness refuses for performance runs and which is never promoted). A failure stops the release.
- `/usr/share/sunnycloud/components.cdx.json` exists and lists every source-built component with name, version and CPE (needed because scanners do not see source-built libraries in the package database, AC-63).
- Source offer: `/usr/share/doc/sunnycloud-decoder/SOURCE-OFFER.txt` and the OCI label `org.sunnycloud.source-offer` point to the release asset `decoder-sources-<version>-<yyyymmdd>.tar.zst` of that exact build (tarballs, patches, recipe; every weekly rebuild publishes its own, see `ci_cd_pipeline.md`) and to `snapshot.debian.org` URLs of the exact Debian source packages (x264 and the others).

**GPU (v1.1)**: an override file `compose.gpu.yml` adds the NVIDIA device reservation to `decoder-general` only, keeps it without a network, and leaves NVENC off unless `SUNNYCLOUD_NVENC=on` (AC-28, gate S10).

### Caddy and Postgres 18

| Property | Caddy | Postgres 18 |
|----------|-------|-------------|
| Image | `caddy:2@sha256:<pin>` | `postgres:18-trixie@sha256:<pin>` |
| Config | `Caddyfile` shipped in the release bundle, mounted read-only; proxies to the API over h2c | `-c` flags from compose: `shared_buffers`, `max_connections=200`, `shm_size: 256m` |
| State | `caddy_data:/data` (certificates; must persist or ACME rate limits bite), `caddy_config:/config` | **`pgdata:/var/lib/postgresql`** (not `/var/lib/postgresql/data`). Postgres 18 images keep data under the versioned `/var/lib/postgresql/18/docker`; mounting the old path silently creates an anonymous volume and loses data on recreate (F20) |
| Ports | 80 and 443/tcp published; no 443/udp (HTTP/3 is out of scope) | 5432 inside backend-net only; published to 127.0.0.1 in dev only |
| Health check | `wget -q --spider http://127.0.0.1:2019/config/` (admin API, container-local; verify against the pinned image) | `pg_isready -U sunnycloud -d sunnycloud` |
| Secrets | none | `postgres_password` via `POSTGRES_PASSWORD_FILE`; init script `initdb.d/10-roles.sh` creates roles `sunnycloud_migrator` (owns the schema; used only by the one-shot `migrate` service) and `sunnycloud` (API runtime: DML only) from secret files. Backups run `pg_dump` and `pg_restore` as the Postgres superuser inside this container (`deployment_procedures.md`) |
| Updates | pulled with each release | minor updates by pulling the `18` image; **never** auto-pull a major; major upgrades are dump/restore (see `deployment_procedures.md`). No Watchtower |

**Caddyfile (structure)**:
```
{
  servers { protocols h1 h2 }          # no HTTP/3
}
{$PUBLIC_HOST} {
  tls { protocols tls1.3 }
  request_body { max_size 64GB }       # well above 2 MiB blocks and 50 GiB single-request ceilings
  reverse_proxy api:8080 {
    flush_interval -1                  # no response buffering; request buffering stays at the default (off)
    transport http {
      versions h2c                     # HTTP/2 prior knowledge to the API, so the API's large HTTP/2 windows apply on the upstream hop
      response_header_timeout 60s      # long-poll waits 30 s
    }
    header_up X-Forwarded-For {remote_host}          # replace, never append: API trusts only TRUSTED_PROXIES
  }
}
```
The Caddyfile has no route to `api:8081` or `:9090`, and Caddy has no interface on the `backend` network, so it could not reach them anyway. HSTS (one year) is emitted by the API, not Caddy, so operators on nginx, Traefik or a tunnel get it too. Operators who bring their own proxy copy the equivalent lines from the install guide: nginx `client_max_body_size 0; proxy_request_buffering off; proxy_buffering off; proxy_read_timeout 120s` (the nginx default 1 MB limit answers 413 to every 2 MiB block); Traefik and Cloudflare Tunnel notes likewise. The block size (2 MiB) stays below Cloudflare's 100 MB request cap.

## Docker networks, volumes and secrets (operator install)

| Name | Kind | Members / mount | Notes |
|------|------|-----------------|-------|
| `proxy` | bridge, fixed subnet `172.29.1.0/24` | caddy (`172.29.1.2`), api | Published ports 80, 443 (caddy). Only the public listener `:8080` is reachable here. The subnet is fixed so `TRUSTED_PROXIES` can name Caddy's address (default deny otherwise, AC-20) |
| `backend` | bridge, fixed subnet `172.29.0.0/24` | api (`172.29.0.2`), db, worker, migrate | Caddy is **not** on this network. `:8081` and `:9090` of the API are bound to its address here. The worker reaches `api:8081` with no TLS inside the network (architecture section 9) |
| (no decoder network) | | decoders use `network_mode: none` | A decoder has no interface but loopback; its only peer is its supervisor through the socket volume (AC-25) |
| `blobs` | bind `${BLOB_PATH}` -> api `/var/lib/sunnycloud/blobs` | api only | XFS (documented default) or ext4; staging dir on the same filesystem; never mounted into worker or decoder |
| `pgdata` | named volume -> db `/var/lib/postgresql` | db | Operator may bind a host path on local SSD, never a network share |
| `caddy_data`, `caddy_config` | named volumes | caddy | |
| `sock_interactive`, `sock_general` | named volumes | worker + the matching decoder | Hold the decoder's Unix socket (mode 0660, shared group); nothing else |
| `scratch_interactive` | named volume with `driver_opts` `type=tmpfs`, `o=size=536870912` (512 MiB, numeric) | worker + `decoder-interactive` | Decrypted photo input and output of one job at a time; RAM-backed. Arithmetic: 256 MiB largest photo input (`worker_max_input_photo`) + 2 outputs of at most 32 MiB = 320 MiB, plus 192 MiB margin = 512 MiB. The tmpfs counts against the 2 GiB `decoder-interactive` limit, so a full tmpfs leaves 1.5 GiB for vips (a 1 GiB tmpfs would leave 1 GiB for no gain). Same value as 09 section 6 |
| `scratch_general` | bind `${SCRATCH_PATH}`: a dedicated mount or a directory under an XFS project quota | worker + `decoder-general` | Decrypted video or photo input and output of one job at a time; ffmpeg reads a scratch file, not a pipe; wiped by the supervisor per job and at start. The mount or quota is the hard cap (at least `worker_scratch_general_cap`, 24 GiB by default); the supervisor's own accounting only refuses jobs early |

| Secret (file) | Mounted in | Notes |
|---------------|-----------|-------|
| `master_key` | api only | Exactly 32 random bytes (`openssl rand -out secrets/master_key 32`); the server refuses a file of any other length |
| `db_api_url`, `db_migrator_url`, `postgres_password` | api / migrate / db respectively | URLs embed the role password (`db_api_url` is role `sunnycloud`; `db_migrator_url` is role `sunnycloud_migrator` and is mounted only into the one-shot `migrate` service); never in env files. There is no worker database secret |
| `setup_token` | api | Optional; if absent the server generates a token on first start with an empty database and prints it once |
| `worker_api_token` | api, worker | Bearer for the internal listener `:8081` (claim, heartbeat, fetch input, post result, fail) |
| `share_grant_key` | api | Key for share access grants (06); the server refuses to start without it |
| `provider_<name>_client_secret` (`github`, `oidc`) | api | |
| `apple_p8` (v1.1) | api | File secret; the server builds Apple's ES256 client secret itself |

Compose file secrets are bind mounts and keep host ownership, so the install guide sets the owner of each file to the uid that reads it (api 10001, migrate 10001, worker 10002, postgres 999) and mode 0400. Verify against the Compose version named in the release notes.

## Docker Compose — Local development (`compose.dev.yml`, repo root)

```yaml
# Outline only
services:
  db:          # postgres:18 digest; ports 127.0.0.1:5432; pgdata-dev volume at /var/lib/postgresql
  caddy:       # Caddyfile.dev: PUBLIC_HOST=sunnycloud.localhost, `tls internal`; request buffering off, same body limit
  migrate:     # same image, command migrate, one-shot, role sunnycloud_migrator
  api:         # build api.Dockerfile, FEATURES=test-clock; env from .env.dev; short durations; secrets from ./secrets-dev (throw-away keys)
  worker:      # build worker.Dockerfile, FEATURES=test-clock
  decoder-interactive:  # build decoder.Dockerfile (first build ~25 min for ffmpeg; cached afterwards; CI publishes a dev-cache image)
  decoder-general:      # same image, disk-backed scratch
  mock-idp:    # build tests/mocks/idp; GitHub-shaped and OIDC-shaped providers; port 9000
volumes: { pgdata-dev, blobs-dev, caddy_data_dev }
```
Fast loop: `docker compose -f compose.dev.yml up db mock-idp caddy` and run `cargo run -p sunnycloud-server` on the host (Postgres exposed on localhost). Blobs in dev live on a plain Docker volume (ext4); performance numbers taken here are not valid for AC-2 or AC-5.

## Docker Compose — Black-box tests (`tests/compose.test.yml`)

The full service, network and volume list is in `tests/environment.md` and is not repeated here. Deployment-side rules for it:
- SUT images come from `SUT_TAG` and are pinned by digest in CI; the API is the `-test` image except in performance runs (AC-1, AC-2, AC-40, AC-43, AC-59), which use the release image.
- The Caddyfile in the stack is the release Caddyfile, not a test copy.
- `worker-supervisor` and the two decoder containers use the same images as the operator install (the supervisor from its `-test` tag) with `cpuset` from `WORKER_CPUSET`; `db` and `api` get the same `cpu_shares` as in the operator install, so contention behaves as in production. The stack has the `migrate` one-shot service, the `proxy` and `backend` networks and no worker database login, as the operator install.
- Run: `tests/run.sh --profile core` (PR gate). Never in quiet mode.
- Dev-fast iteration: bind-mount `tests/` into the consumer image; rebuild SUT images only when a Dockerfile or manifest changes.

## Docker Compose — Operator install (`deploy/compose.yml`, released as one file)

```yaml
# Outline only; one file, no overlays required
name: sunnycloud
x-logging: &logging { driver: json-file, options: { max-size: "10m", max-file: "5" } }
x-hardened: &hardened { cap_drop: [ALL], security_opt: ["no-new-privileges:true"], restart: unless-stopped, init: true, logging: *logging }
secrets: { master_key, db_api_url, db_migrator_url, postgres_password, worker_api_token, share_grant_key, provider_github_client_secret, provider_oidc_client_secret }  # file: ./secrets/<name>
networks: { proxy: { ipam: { config: [{ subnet: 172.29.1.0/24 }] } }, backend: { ipam: { config: [{ subnet: 172.29.0.0/24 }] } } }
volumes: { pgdata, caddy_data, caddy_config, sock_interactive, sock_general, scratch_interactive (tmpfs), scratch_general }
services:
  caddy:    # image digest; network proxy only (172.29.1.2); ports 80,443; cap_add NET_BIND_SERVICE; Caddyfile ro; env PUBLIC_HOST; healthcheck; depends_on api healthy
  migrate:  # image sunnycloud-api@digest, command ["migrate"]; one-shot (restart: "no"); network backend; secret db_migrator_url only; depends_on db healthy
  api:      # image sunnycloud-api@digest; one instance (no replicas); read_only; tmpfs /tmp; blobs bind; secrets master_key, db_api_url, worker_api_token, share_grant_key, providers, setup_token(optional)
            # networks proxy (172.29.1.x) and backend (172.29.0.2); listeners 8081 and 9090 bound to the backend address; cpu_shares: 2048; ulimits nofile 1048576; stop_grace_period 120s; env SUNNYCLOUD_* from .env
            # depends_on db healthy and migrate service_completed_successfully (the API never migrates itself)
  db:       # image digest; network backend; pgdata:/var/lib/postgresql; secrets; healthcheck pg_isready; stop_grace_period 60s; shm_size 256m; cpu_shares: 2048
  worker:   # image sunnycloud-worker@digest; network backend; mem_limit 512m; mounts sock_interactive, sock_general, scratch_interactive, scratch_general; secret worker_api_token only; NO database secret, NO blobs, NO master_key
  decoder-interactive:  # image sunnycloud-decoder:${SUNNYCLOUD_VERSION} (moving tag, see Image tagging); network_mode none; read_only; mounts sock_interactive + scratch_interactive; no secrets
            # cpus: 2, mem_limit/memswap_limit: 2g, pids_limit: 512, ulimits nofile 4096, user 10003
  decoder-general:      # same image; network_mode none; read_only; mounts sock_general + scratch_general; no secrets
            # cpus: ${WORKER_CPUS:-2}, mem_limit/memswap_limit: ${WORKER_MEM:-4g}, pids_limit: 512, ulimits nofile 4096, user 10003; scratch_general is the bind ${SCRATCH_PATH}
```
The release `compose.yml` pins caddy, db, api, migrate and worker by digest (written from `images.lock` at release). The decoder is the one deliberate exception: it is referenced by the moving `:<semver>` tag so that the weekly security rebuild reaches operators without a new release (section Image tagging).

**Decoder resource limits (AC-25)**: the per-job limits are photos 2 CPU / 2 GiB / 120 s and video 4 CPU / 4 GiB / 40 min, applied by `decoder-runner` around each subprocess (`VIPS_CONCURRENCY=2`, `ffmpeg -threads 4`, wall-clock kill, memory limit by cgroup or rlimit as S4/S10 prove workable; `RLIMIT_AS` below 4 GiB breaks NVENC in v1.1). The container limits are the outer bound: `decoder-interactive` is sized for one photo job (2 CPU, 2 GiB, fixed) and `decoder-general` for one video job (`WORKER_CPUS` default 2, `WORKER_MEM` default 4g). **CPU defaults**: `WORKER_CPUS` defaults to 2 and must not exceed half the host cores, so the database and the API keep their share (`cpu_shares: 2048` on `db` and `api` against the default 1024 on the workers; an operator may instead pin them with `cpuset`). A host with 8 or more cores can raise `WORKER_CPUS` to 4 for faster video transcodes. The reserved interactive container keeps its slot (AC-10). The stalled-ffmpeg watchdog (kill 90 s after the last progress line, one CPU retry, AC-45) lives in the runner.

**More workers**: add another set of `worker`, `decoder-interactive` and `decoder-general` on the same host (copy the three service blocks with their own socket and scratch volumes); each decoder container still handles one job at a time, so one job's decrypted input is invisible to another. **Workers on other hosts are unsupported in v1.0**: `:8081` is plain HTTP with a bearer token on the internal `backend` network and is never published.

**Install layout** (documented, not scripted): `/opt/sunnycloud/{compose.yml,Caddyfile,.env,secrets/}`, blobs on a dedicated XFS mount, `secrets/` mode 0700.

## Image tagging

| Context | Tag | Mutable |
|---------|-----|---------|
| CI on any push | `<image>:sha-<git-sha>` | no |
| `stage` branch | `<image>:stage-<git-sha>` and `:stage` | `:stage` moves |
| `main` branch | `<image>:main-<git-sha>` | no |
| Test image | `<api>:<SUT_TAG>-test` | no; never promoted |
| Release | `<image>:<semver>` (api, worker), recorded with its digest in `images.lock`; the release compose references the digest | no |
| Decoder release | `:<semver>-<yyyymmdd>` (immutable) and `:<semver>` (moves to the newest weekly rebuild of that version; the release compose uses this tag). Each rebuild also publishes `decoder-sources-<semver>-<yyyymmdd>.tar.zst`, the GPL source of that exact build | `:<semver>` moves |
| Never | `:latest` is not published | |

## .dockerignore

```
.git
_docs
node_modules
target
tests/e2e-results
**/.env*
secrets*
*.md
```
(`README.md` is re-included for the api image only if the CLI embeds it; otherwise excluded.)

## AC coverage

| AC | Where addressed |
|----|-----------------|
| AC-22 | Single compose file, Caddy default, `sunnycloud doctor` printing redirect URIs, secrets layout above |
| AC-24 | Scrub runs as a janitor inside `sunnycloud serve` (throttled); no extra container; metrics in `observability.md` |
| AC-25 | Decoder: own image, `network_mode: none`, no secrets, no blob mount, read-only, limits, compile-time trimming, red-team via `e2e-ctl inspect` |
| AC-43 | `master_key` only in the API container, never in worker or decoder; not on the blob volume |
| AC-63 | Digest-pinned bases, source-built component SBOM, source offer, weekly rebuild (`ci_cd_pipeline.md`) |
| AC-64 | Version labels and `GET /api/v1/server-info` come from `VERSION`/`REVISION` build args |
| AC-65 | Postgres 18 volume layout; `pgdata` and `blobs` are the two data volumes the backup covers |

## Resolved and open items

| # | Item | Status |
|---|------|--------|
| O-1 | Supervisor-to-decoder transport | Resolved (architecture section 9): Unix socket in a shared volume, one job at a time per decoder container, no Docker socket |
| O-2 | Internal API endpoint transport | Resolved (decision 13, Step 4 S4-1): second API listener `:8081`, plain HTTP with a bearer token on the backend network; the worker has no database login |
| O-3 | Web bundle served by the API (assumed here) or by a static container | API-served: one artefact, one version |
| O-4 | Arm64 images | amd64 only in v1.0 (REF-SRV is amd64); arm64 on GitHub's arm runners when the owner asks |
| O-5 | Remote workers on other hosts | Closed (Step 4 S4-1): unsupported in v1.0 |
