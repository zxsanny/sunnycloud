# sunnycloud — Deployment Procedures

**Status**: draft (planning, no code yet)
**Date**: 2026-11-04
**Audience**: the operator who installs and runs a sunnycloud server, plus the maintainers who write the install guide from this document.
**Sources**: `architecture.md`; `restrictions.md`; `b_ops_licence_scope.md` (F1, F7, F8, F12, F20); AC-22, AC-23, AC-24, AC-25, AC-30, AC-41, AC-43, AC-63, AC-64, AC-65.

## Deployment strategy

- **All-at-once**, one Docker Compose project per installation: `docker compose pull && docker compose up -d`. No blue-green, canary or orchestrator in v1.0; exactly one API process (the architecture scales the API vertically): upload progress is held in memory, so replicas are unsupported for uploads (decision 12).
- **Compatible with the CI model**: nothing deploys remotely from CI. CI produces signed images and a release bundle; each operator pulls them.
- **Versioning**: the server and its API are versioned separately. The API lives under `/api/v1`. The server publishes `min_client_version`, `api_versions` and its limits at `GET /api/v1/server-info`; clients send `X-Sunnycloud-Client: <family>/<semver>`. A client below the minimum receives HTTP 426 with the stable error code `upgrade_required` carrying the minimum version, and shows an upgrade message instead of misbehaving (AC-64). Compatibility promise: server N supports clients N and N-1; `min_client_version` rises only when a protocol or format change requires it and the release notes say so.
- **Graceful shutdown**: the API gets `SIGTERM` and 120 s. It stops accepting new sessions, lets in-flight blocks finish and fsync, closes held long-polls (clients reconnect), stops the janitor loops that run inside `sunnycloud serve`, and exits. Uploads cut off by the deadline resume from the received-bitmap (AC-3). Workers finish or release their lease (5 min); an unreleased lease expires and the job retries. Postgres gets 60 s.
- **Migration ordering**: the one-shot `migrate` service (role `sunnycloud_migrator`, `db_migrator_url`) runs the migrations before the API starts, each in a transaction, under an advisory lock; the API container waits for it (`service_completed_successfully`). The API runs as `sunnycloud` (DML only) and never migrates: at start it checks the schema version and refuses to serve on a mismatch. Workers have no database login; they start after the API is ready and talk only to the API on `:8081`. Old API instances never run beside new ones (single instance).

## Health checks

| Check | Command | Healthy |
|-------|---------|---------|
| Container (api) | `sunnycloud healthcheck` -> `127.0.0.1:9090/readyz` | 200 within 120 s of start |
| Container (worker, `decoder-interactive`, `decoder-general`, db, caddy) | see `containerization.md` | healthy |
| Public | `curl -fsS https://$PUBLIC_HOST/api/v1/server-info` | 200, expected `version` |
| Deep | `docker compose exec api sunnycloud doctor` | exit 0 (1 = read the warnings) |
| After DB outage | `docker stop db`, then `start` | API ready within **60 s** of the database accepting connections |

`/readyz` returns 503 with the failing checks while the database is down, and the API answers 503 to requests that need it; clients retry. When the identity provider is unreachable, existing sessions and device tokens keep working and only new sign-ins fail (AC-21).

## First run (AC-22, AC-23)

Target: working server in at most **30 minutes** for an operator with one sign-in provider ready, DNS pointing at the host, and ports 80 and 443 reachable. Indicative budget: prerequisites and bundle 5 min, secrets and `.env` 5 min, image pull 5 min, start 2 min, provider registration 5 min (GitHub about 2), first sign-in and checks 5 min, slack 3 min. The timed `MAN` test measures it.

1. **Prerequisites**: Linux host with a current Docker Engine and Compose v2 (versions tested are in the release notes), a dedicated XFS (or ext4) mount for blobs with enough space, a DNS name, ports 80/443 open, one OAuth app or OIDC client registered with the redirect URI printed in step 6.
2. **Download and verify** the release bundle `sunnycloud-deploy-<ver>.tar.gz`; verify the SHA-256 and the cosign signature (commands in the guide). Unpack to `/opt/sunnycloud`.
3. **Create secrets** (`umask 077`): `master_key` (`openssl rand -out secrets/master_key 32`), Postgres password and the two database URLs (`db_api_url` for role `sunnycloud`, `db_migrator_url` for role `sunnycloud_migrator`), `worker_api_token`, `share_grant_key`, the provider client secret, optionally `setup_token`. Set each file's owner to the uid that reads it (table in `containerization.md`).
4. **Back up the master key now**, into two separate places, before any data exists (rule in `environment_strategy.md`). The guide states that losing it makes all data unrecoverable.
5. **Edit `.env`**: `PUBLIC_HOST`, `SUNNYCLOUD_PUBLIC_URL`, `BLOB_PATH`, `SCRATCH_PATH` (a dedicated mount or an XFS project quota), `WORKER_CPUS` (at most half the host cores), provider IDs, quota default, free-space reserve.
6. **Print redirect URIs**: `docker compose run --rm api sunnycloud doctor --pre-flight` prints the exact redirect URIs to register for each enabled provider and checks config, key length and the blob volume. Register them, copy the client ID and secret into `.env` and `secrets/`.
7. **Start**: `docker compose up -d`. The `migrate` service runs once after Postgres is healthy, then the API starts. `docker compose ps` shows all services healthy within about two minutes.
8. **Setup token and first admin**: with an empty database, no account is created by a social login unless it presents the one-time setup token. If `setup_token` was supplied, use it; otherwise the API generated one and printed it once in `docker compose logs api` (the single line that contains it). Open `https://$PUBLIC_HOST`, choose the provider, and enter the token on the empty-installation sign-in screen. That identity becomes the first admin and the token is voided; a second use fails (AC-23). Remove the plain secret file, if one was used.
9. **Check**: `docker compose exec api sunnycloud doctor` reaches exit 0 (proxy, TLS 1.3, HTTP/2, key, volume, providers). Upload a small file and a photo from the web app; the thumbnail appears within seconds (decoder path works).
10. **Invite others**: admin section or `docker compose exec api sunnycloud admin invite create` yields a single-use invite token; registration never matches by email except at invite redemption with a provider-verified address.
11. **Schedule backups** now (next section) and write the date of the first successful restore drill in the operator's notes.

Operator commands (names owned by `12_operator_cli`): under `sunnycloud admin`: `create`, `reset`, `promote`, `demote`, `setup-token` (AC-23: create or reset admins without the web), `invite create|list|revoke`, `user list|set-quota|delete`, `epoch-bump`, `restore`, `rewrap`, `keycheck-init`, `gc-run`, `media-requeue`, `media-stats`, `audit export`; top level: `scrub run [--full]`, `scrub status`, `migrate`, `doctor`, `key-check`, `healthcheck`, `serve`, `openapi`, `version`.

### Reverse proxy variants

Default is the Caddy in the Compose file. For an operator's own proxy, the guide lists the required lines: no request buffering, no response buffering for downloads and long-poll, body size limit at least 2 MiB (set unlimited for single-request and block uploads), read timeout above 30 s, HTTP/2 to the client and h2c to the API's public listener `:8080` (never route `:8081`), `X-Forwarded-For` set by the proxy, and `SUNNYCLOUD_TRUSTED_PROXIES` set to the proxy's address. nginx: `client_max_body_size 0; proxy_request_buffering off; proxy_buffering off; proxy_read_timeout 120s;`. Traefik and Cloudflare Tunnel have equivalent notes (Cloudflare's terms for large transfers are unchecked; the guide does not call it supported until they are).

## Upgrade between releases (AC-64, AC-65)

1. **Read the release notes**: breaking changes, new `min_client_version`, migration duration, tested Docker/Compose versions.
2. **Back up first**: a fresh database dump (below) and confirm the blob backup is current. Always, even for patch releases.
3. **Pull**: set `SUNNYCLOUD_VERSION=<new>` in `.env`, `docker compose pull`. Verify signatures if the guide's `cosign verify` snippet is part of the operator's routine.
4. **Apply**: `docker compose up -d`. The `migrate` service runs the migrations first (advisory lock, one transaction), then the API starts. A migration interrupted by a crash or kill rolls back; the API then refuses to serve (schema version mismatch) until `migrate` has run, and `docker compose up -d` again re-runs it (AC-65).
5. **Verify**: `doctor` exit 0, `/api/v1/server-info` shows the new version, upload and download a file, a device syncs. Watch `docker compose logs api` for a few minutes.
6. **Clients**: if `min_client_version` rose, older clients show the upgrade message. Roll apps through the stores with a staged rollout; desktop clients update through their signed updater (source fixed at build time, never this server, AC-49).
7. **Never auto-update**: no Watchtower or similar. A Postgres **major** upgrade is a separate dump-and-restore procedure in the release notes, not part of a normal upgrade; the Postgres 18 data path is `/var/lib/postgresql` and the compose file keeps it.
8. **Decoder rebuilds**: between releases a patched decoder image appears weekly (`:<semver>` moves). `docker compose pull worker decoder-interactive decoder-general && docker compose up -d worker decoder-interactive decoder-general` takes it without touching the API. The guide recommends doing this weekly; `doctor` warns after 14 days (AC-63).

**Rollback**: production migrations are forward-only (`down` files are for CI and recovery, not for operators). To go back to the previous release, stop the stack, restore the pre-upgrade database dump and blob state (see Backup and restore), set `SUNNYCLOUD_VERSION` to the previous release, run `sunnycloud admin epoch-bump` so devices resync, and start the stack. The API checks the schema version at start and refuses to serve on a mismatch, so pointing `SUNNYCLOUD_VERSION` back without restoring is not supported. A rollback always costs the changes made since the backup.

**CI proof (AC-65)**: `ops-drill` upgrades from the previous release images, with data present, to the candidate on every `stage`/`main` push, and checks that all data, shares and accounts survive.

## Backup and restore (AC-24, AC-30, AC-41, AC-43, AC-65)

What must be backed up, in this order of importance:

| Item | Where it lives | How | Notes |
|------|----------------|-----|-------|
| Master key | `secrets/master_key` | Two offline copies, separate from everything below | Without it the other two are useless |
| Database | Postgres `pgdata` | `docker compose exec -T db pg_dump -U postgres -Fc sunnycloud > db-<date>.dump` (the Postgres superuser inside the db container; single consistent snapshot) | Includes tree, sessions, audit, wrapped blob keys. Roles are not in the dump: the init script recreates them on an empty data directory. A physical or WAL-archive backup is also fine but not required |
| Blobs | `${BLOB_PATH}` | `restic`, `rsync` or a ZFS/btrfs/LVM snapshot, **started after the dump finishes** | Immutable files; the same encrypted bytes, safe to copy while running |
| Config | `.env`, `Caddyfile`, `compose.yml`, `secrets/` except the master key | Any file backup, encrypted | Provider secrets can also be re-created in the provider console |

**Ordering rule**: dump the database first, then copy the blobs. Blobs are written before the database commit, so this order can only yield blobs the dump does not know about (orphans, swept after 48 h), never a database that points at missing blobs. The reverse order is wrong.

**GC and the blob copy**: GC runs while the blobs are being copied and may unlink retained blobs whose retention falls due between the dump and the end of the copy. The restored database would still point at those blobs, so `scrub` would report them missing. Keep `SUNNYCLOUD_RETENTION` minus `SUNNYCLOUD_GC_INTERVAL` larger than the time the blob copy takes (the longest blob copy seen, plus slack). With the 7-day retention and the 5-minute GC interval this only bites on very slow copies or on a short retention.

**Retention interplay (7 days)**: replaced and deleted blobs are kept for `SUNNYCLOUD_RETENTION` (default 7 days, 0 to 90) through the GC queue. This is what makes a restored older database consistent: it can point at blobs removed from the user's view up to 7 days before the dump, and those blobs still exist. Rules:
- Keep the **backup interval shorter than the retention** (daily backups with 7 days leaves slack). `doctor --backup-interval` checks the setting.
- **A backup older than the retention** may reference blobs that GC already removed. After restoring such a backup `scrub` reports them as missing and lists the affected files; those files must be re-uploaded from devices. Do not rely on a backup older than the retention for anything but disaster recovery.
- Restoring a file deleted by mistake without a full restore: `docker compose exec api sunnycloud admin restore --account <id> --path <path> --at <time>` works while the blob is inside the retention window. The file returns to its original folder; if that folder was deleted, it goes into a folder named `Restored` at the root of the account (decision 4). Account deletion is immediate and not recoverable this way (AC-27).
- Setting retention to 0 turns the safety net off; then a snapshot-capable file system is the only protection.

**Restore procedure** (onto a clean host or after loss; also the CI drill):
1. Install the same release as the backup (or the next one); keep the stack stopped.
2. Restore `secrets/` including the **same master key**. A different key cannot read the blobs; the API refuses to start if the key does not match the installation (key-check value). If `.keycheck` was lost with the volume and the key is confirmed correct, run `sunnycloud admin keycheck-init` once.
3. Start only the database: `docker compose up -d db`. Restore: `docker compose exec -T db pg_restore -U postgres -d sunnycloud --clean --if-exists < db-<date>.dump` (superuser; ownership goes back to `sunnycloud_migrator`, which the init script created on the empty data directory).
4. Restore the blobs to `${BLOB_PATH}` (same layout `blobs/<2hex>/<2hex>/<id>`), owner uid 10001.
5. **Bump the epoch**: `docker compose run --rm api sunnycloud admin epoch-bump`. It sets a new epoch for every account and writes the `.epoch` marker on the blob volume; `doctor` reports **Fail** when the marker is missing or differs from the database epoch, and `sunnycloud serve` refuses to start while the database epoch is below the `.epoch` marker (it prints `admin epoch-bump`), so a forgotten bump cannot go live (decision 5). Devices then receive `resync_required` and converge: local-only files upload, server-only files download, nothing is deleted except through the mass-delete guard (AC-41).
6. Run `docker compose run --rm api sunnycloud scrub run --full` and expect zero mismatches and zero missing blobs for a backup inside the retention window.
7. `docker compose up -d`, then `doctor`, then a device sync test.

**Restore drill**: the operator repeats this on a spare host at least once after install and after each major release. CI performs it on every `stage`/`main` push and every release with the exact commands above (`pg_dump` and `pg_restore` as the Postgres superuser in the db container): dump, wipe, restore, scrub, compare file hashes against the pre-wipe listing (AC-65). A CI drill that cannot run is `NOT-RUN`, never PASS.

**Key rotation** (AC-43): `sunnycloud admin rewrap --new-key-file ...` rewraps per-blob keys in the database without rewriting blobs. Keep the previous key until every retained backup that predates the rotation has expired, because those dumps hold keys wrapped under it.

**Scrub (AC-24)**: runs as a janitor inside `sunnycloud serve` on the configured interval, throttled so transfers keep at least 90% of their baseline; each run leaves a `scrub_summary` audit event and metrics (no table). Any mismatch raises an alert; the repair is: restore that blob from the blob backup (same ID, same bytes), run `sunnycloud scrub run`, and file a report.

## Scratch sizing and worker secret rotation (E-20)

**Scratch sizing** (arithmetic in `components/09_worker_supervisor/description.md` section 6):
- `scratch_interactive` is a numeric tmpfs of 512 MiB (`size=536870912`): 256 MiB largest photo input + 2 x 32 MiB outputs = 320 MiB, plus 192 MiB margin. It counts against the 2 GiB `decoder-interactive` limit, so never raise it above about 1 GiB; raising `worker_max_input_photo` needs the same sum redone.
- `scratch_general` is the dedicated mount or XFS project quota at `${SCRATCH_PATH}`: 24 GiB = 2 x `worker_max_input_video` (10 GiB, input plus a remux/transcode output no larger than the input) + 4 GiB margin, one mount per supervisor/decoder pair. If an operator lowers `worker_max_input_video`, the mount may shrink by 2 GiB per GiB removed; if the mount is smaller than the sum, the supervisor refuses big inputs early (`input_too_large`, permanent) and the original is still served.
- **Gate S5 revisit**: the 24 GiB figure is a bound, not a measurement. At S5, measure peak scratch use for the largest real clip and for a remux of a 10 GiB file, then confirm or lower it; the value stays 24 GiB until S5 reports.

**Worker secret rotation** (no window in which the worker is refused; the API accepts `worker_api_token` and `worker_api_token_next` together, 08 `WorkerAuthenticator`):
1. Create the new secret (`umask 077; openssl rand -hex 24 > secrets/worker_api_token_next`), declare it in `compose.yml` and mount it into the **api** only, then `docker compose up -d api`. The API now accepts the old and the new value; the running worker is unaffected (its calls during the API restart end as `Transient` and retry).
2. Point the **worker's** `worker_api_token` secret (target path `/run/secrets/worker_api_token`) at the new file, keeping the owner uid 10002 and mode 0400.
3. Restart the worker: `docker compose up -d worker` (secrets are read at start; running jobs get `worker_shutdown_grace` and are re-queued by lease expiry otherwise). Check `docker compose logs worker` shows no 403, `/readyz` on `:9090` is 200 and `doctor` shows a fresh worker status. A 403 makes the worker stop claiming until restarted (AC-79), so a wrong value shows up at once.
4. Finish: move the new value into `secrets/worker_api_token`, remove `worker_api_token_next` and its mounts, restore the worker's source, then `docker compose up -d api worker`. With more than one worker set, finish steps 2 and 3 for every set before step 4.

## Rollback

Rollback after a committed migration is a database restore, not `migrate revert` (`down` files exist for CI and recovery only). Rollback means returning to the previous images, and to the previous database if a migration was committed.

| Situation | Action |
|-----------|--------|
| Trigger | `/readyz` failing for more than 5 min after start; `doctor` exit 2; smoke test fails; a critical regression found within the first hours |
| New version failed **before** its migration committed (interrupted or rolled back) | Set `SUNNYCLOUD_VERSION` back, `docker compose up -d`. The database is unchanged. Blobs written in the window are harmless orphans |
| Migration committed, new version is bad | 1. `docker compose down` (volumes stay). 2. Restore the pre-upgrade dump into a **new** `pgdata` volume (steps 3 and 5 of the restore). 3. Set the previous version, `up -d`. 4. Keep the old volume until the rollback is verified |
| What devices see | The epoch bump gives `resync_required`; changes made on devices during the failed window are re-uploaded as local-only files, not lost. Clients newer than the server show "server too old" rather than corrupting anything |
| Blobs | Never rolled back. Anything written in the failed window is an orphan swept after 48 h or kept by GC rules |
| Master key | Unchanged. If a rotation was part of the failed upgrade, use the previous key and the matching dump |
| Post-mortem | Record cause, timeline, whether the backup was the limiting factor; file the upgrade issue before the operator retries |

## Local / dev and staging

- **Dev**: `docker compose -f compose.dev.yml up`; `https://sunnycloud.localhost` with Caddy's internal CA; sign in through mock-idp; `POST /test/clock` (`:9091`) available, and the dev supervisor follows it through `GET /internal/v1/test/clock-offset`.
- **Test**: `tests/run.sh --profile core`; reset with `tests/run.sh reset` (under 60 s).
- **Staging**: there is no deploy target. The `stage` branch produces signed images and runs `core`, `ops-drill` and the nightly suites; the evidence is the CI run. If the owner provisions a staging host later, it follows the first-run procedure with `stage-<sha>` images and a throw-away master key.

## Release checklist (maintainer)

- [ ] Main is green; last nightly green or each failure waived in writing
- [ ] `deny.toml` ignores have `added`/`expires`, none older than 90 days (AC-63)
- [ ] Dependency floors hold (tokio >= 1.54.1, h2 >= 0.4.16, bytes >= 1.11.1, rustls >= 0.23.45, libheif >= 1.22.0)
- [ ] Decoder rebuilt within 7 days; 0 HIGH/CRITICAL in ffmpeg, libheif, libvips, libwebp; ffmpeg has no nonfree build, no libfdk_aac
- [ ] Images signed (digest), SBOM and provenance attached, `images.lock` written
- [ ] The pushed `@digest` of the api image and of the worker image returns 404 for both `POST /test/clock` and `GET /internal/v1/test/clock-offset`, asserted in `release.yml` before signing (also in `images`)
- [ ] `ops-drill` passed: upgrade from the previous release, backup-restore, interrupted-migration rollback
- [ ] Old-client test against `/api/v1` passes; `min_client_version` decided and written in the notes
- [ ] Timed install (`MAN`, AC-22) done on a clean VM with only the README: <= 30 min
- [ ] Source offer asset `decoder-sources-<ver>-<yyyymmdd>` of the shipped decoder build attached; `LICENSE` and `COPYING.iOS` in place
- [ ] Release notes: breaking changes, migration time, tested Docker/Compose, Postgres image digest, how to rollback
- [ ] Desktop installers signed (decision D-1 resolved or the notes say unsigned); update manifest signed offline with the active updater key (`serial` raised, `expires` set, per-OS SHA-256 matching the uploaded payloads) and verified against both pinned public keys; Android AAB uploaded to the Play staged rollout
- [ ] Lab matrix results attached or each NOT-RUN waived (H-WIN, H-LIN, H-ANDROID, Safari)

## Operator checklist (per install)

- [ ] Master key created, two separate offline copies, restore drill done once
- [ ] Blob volume on XFS (or ext4), free-space reserve set, quota default set
- [ ] DNS, ports 80/443, `PUBLIC_HOST` correct; TLS 1.3 and HTTP/2 confirmed by `doctor`
- [ ] At least one provider enabled; redirect URIs registered; first admin created; setup token gone
- [ ] Backups scheduled: dump first, blobs second, interval shorter than retention; config backed up
- [ ] Monitoring pointed at `:9090/metrics` or at least `doctor` run by cron; alerts for disk, scrub, queue
- [ ] Weekly pull of the decoder image; release notes read before each upgrade

## AC coverage

| AC | Procedure |
|----|-----------|
| AC-22 | First run: ten steps, `doctor --pre-flight`, timed `MAN` test |
| AC-23 | Setup token step, `admin` commands, invite flow |
| AC-24 | Scrub schedule, throttle, repair of a mismatch |
| AC-25 | Worker start-up isolation self-check; decoder limits in `containerization.md`; checklist item on the decoder image |
| AC-30 | Retention interplay, `sunnycloud admin restore`, backup interval rule |
| AC-41 | Restore step "bump the epoch", `serve` refusal, device convergence |
| AC-43 | Master-key creation, backup, drill, rotation |
| AC-63 | Weekly decoder pull, release checklist (scan, SBOM, signing, floors) |
| AC-64 | `/api/v1`, `min_client_version`, 426 `upgrade_required`, N and N-1 promise |
| AC-65 | Upgrade, backup, restore, rollback, and the CI `ops-drill` that proves them |

## Resolved items (2026-11-04)

| # | Item | Resolution |
|---|------|------------|
| P-1 | Epoch bump after a database restore | Explicit `admin epoch-bump`; `doctor` detects a missing bump from the `.epoch` marker (decision 5) |
| P-2 | Master-key check value at first start | Required; `.keycheck`, `admin keycheck-init` for a restored volume |
| P-3 | Subcommand and flag names | Owned by `12_operator_cli`; the lists above follow it |
| P-4 | Short-outage behaviour of sessions while Postgres is down | API answers 503 and clients retry; the 60 s recovery budget (restrictions) is measured by `e2e-ctl` |

Supervisor to decoder uses a Unix socket in a shared volume, one decoder container per lane (interactive, general); the internal API listener `:8081` uses a bearer token over the private network without TLS; Caddy proxies to the API over h2c; the restore procedure uses an explicit `epoch-bump`; a master-key check value is stored at first start (architecture.md section 9). Signing and registry choices: GHCR only, amd64 only, SignPath Foundation for Windows in v1.0.
