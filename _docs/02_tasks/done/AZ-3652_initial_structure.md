# Initial Project Structure

**Task**: AZ-3652_initial_structure
**Name**: Initial Structure
**Description**: Scaffold the Cargo workspace root and every skeleton crate under `src/`, with the crate-graph test; record the full skeleton plan that the other E-00 children and all later epics follow
**Complexity**: 5 points
**Dependencies**: None
**Component**: Bootstrap
**Tracker**: AZ-3652
**Epic**: AZ-3621

## Scope of this task

This task implements E-00 child issues 1 and 2 only. The other sections describe the whole skeleton so later tasks follow one plan.

- **Child 1 (workspace root)**: `src/Cargo.toml`, `src/Cargo.lock`, `src/rust-toolchain.toml`, `src/rustfmt.toml`, `src/deny.toml`, `src/.cargo/config.toml`, workspace lint set, root `.gitignore` additions.
- **Child 2 (skeleton crates)**: every crate in the crate table below as an empty library or a `main` that does nothing; the `sc_db::DbTx` type with no public constructor; the crate-graph test.
- **Complexity**: the epic estimates 3 + 3. Both children are mechanical (manifests, empty crates, one test) and share one review and one set of name decisions, so 5 is enough. No split.
- **Out of scope here**: port signatures, `media-shared` types, client traits, harness, CI, migrations, Dockerfiles, LICENSE (the ten later E-00 children, see Implementation Order).
- **Names**: crate and folder names follow `_docs/02_document/module-layout.md`. E-00 risk 4 says names are confirmed in this first task, so the BLOCK on this plan also confirms that file's "Open layout points".

## Project Folder Layout

```
sunnycloud/                         repository root
├── .github/workflows/ci.yml        PR gate (CI skeleton child); .github/dependabot.yml
├── .gitignore  .dockerignore  LICENSE  README.md
├── deploy/
│   ├── docker/                     api.Dockerfile, worker.Dockerfile, decoder.Dockerfile
│   ├── postgres/initdb.d/          10-roles.sh: creates sunnycloud_migrator and sunnycloud
│   └── compose/                    compose.dev.yml, .env.example (release: compose.yml, Caddyfile)
├── scripts/ci/                     audit.sh, watchdog.sh (later: check-decoder-image.sh, ...)
├── tests/                          black-box harness (see Test Structure)
└── src/                            Cargo workspace root
    ├── Cargo.toml  Cargo.lock  rust-toolchain.toml  rustfmt.toml  deny.toml  .cargo/config.toml
    ├── .sqlx/                      committed sqlx offline query data
    ├── migrations/                 sqlx reversible pairs (empty in E-00)
    ├── contracts/                  server-only port traits and shared DTOs
    ├── media-shared/               job DTOs, transcode policy, decoder protocol; no sqlx, no I/O
    ├── sync-core/  sync-core-wasm/ 13: native client and client traits; wasm32 adapter
    ├── sync-core-proto/            13: sans-IO protocol part (native and wasm32): created by E-23, not here
    ├── shared/{config,error,clock,audit,ids_hashing,names,db,ratelimit,observability}/
    ├── components/{01_identity,02_tree,03_transfer,04_blobstore,05_sync_feed,06_sharing,
    │               07_admin,07a_quota,08_media_jobs,11_http_api,20_vault_service}/
    ├── server/                     12: package `sunnycloud-server`, binary `sunnycloud` (composition root, CLI)
    ├── worker/  decoder/           09 `sunnycloud-worker`; 10 `decoder-runner`
    ├── desktop/{daemon,settings,ipc_schema}/   14, 15
    ├── mobile_backup/  vault-core/ 17/18 shared uploader; 19 vault crypto (v1.2)
    └── web/  mobile/               Svelte 5 app (E-26) and Flutter app (E-27/E-28): not created here
```

### Crates created by child 2

| Folder (`src/...`) | Package (proposed) | Kind | Component | Edges declared now |
|---|---|---|---|---|
| `shared/<concern>/` | `sc_config`, `sc_error`, `sc_clock`, `sc_audit`, `sc_ids` (folder `ids_hashing`), `sc_names`, `sc_db`, `sc_ratelimit`, `sc_observability` | lib | helpers 01-08; E-09 stub | none |
| `contracts/` | `contracts` | lib | shared, server only | `sc_db` (only for `DbTx`) |
| `components/NN_<name>/` | `sunnycloud-identity`, `-tree`, `-transfer`, `-blobstore`, `-sync-feed`, `-sharing`, `-admin`, `-quota`, `-http-api`, `-vault-service`; 08 (`08_media_jobs/`): `media-api` | lib | 01-08, 07a, 11, 20 | `contracts` (08 also `media-shared`) |
| `server/` | `sunnycloud-server`, bin `sunnycloud` | bin | 12 | every component, `contracts` |
| `media-shared/` | `media-shared` | lib | 08, 09, 10 | none |
| `worker/`, `decoder/` | `sunnycloud-worker`, `decoder-runner` | bin | 09, 10 | `media-shared` |
| `sync-core/`, `sync-core-wasm/` | same names | lib | 13 | none yet: E-23 adds `sync-core-proto` under both; the wasm crate never depends on `sync-core` (no tokio) |
| `desktop/daemon`, `desktop/settings`, `desktop/ipc_schema` | `sunnycloud-daemon` (proposed; the docs fix no daemon name), `sunnycloud-settings`, `ipc_schema` | bin, bin, lib | 14, 15 | daemon -> `sync-core`, `ipc_schema`; settings -> `ipc_schema` |
| `mobile_backup/`, `vault-core/` | same names | lib | 17/18, 19 | `mobile_backup` -> `sync-core` |

Helper edges are added by each epic when it first uses a helper. Every crate has `[lints] workspace = true` and inherits `rust-version` and `license` from `[workspace.package]`.

### Workspace root files (child 1)

- `src/Cargo.toml`: `[workspace]` with every member above; `[workspace.package]` `rust-version = "1.88"`, `license = "AGPL-3.0-only"`; edition chosen at implementation (MSRV 1.88 allows 2024).
- `[workspace.dependencies]` pins, copied from E-00 Architecture notes: axum 0.8.9, hyper 1.12 (+ hyper-util), tokio >= 1.54.1, h2 >= 0.4.16, bytes >= 1.11.1, rustls >= 0.23.45, sqlx 0.9, blake3 1.8.7, `chacha20poly1305 =0.11.0`, `aead-stream =0.6.0`, openidconnect 4.1.0, oauth2 5.0.0, uniffi 0.32.2, notify 8.2.1, tray-icon 0.26.1, slint 1.18.1, rfd 0.17.2, rusqlite 0.41.1, reqwest 0.13.5, `wasm-bindgen =0.2.130`. Any other crate (hyper-util, serde, serde_json, thiserror): pinned at implementation from `[workspace.dependencies]`. jsonwebtoken is pinned by E-15. Unused entries do not enter `Cargo.lock` until a crate uses them.
- `[workspace.lints]`: `unused_must_use = "deny"` and `clippy::let_underscore_must_use = "deny"` (E-02 silent-catch gate); CI adds `-D warnings`.
- `src/rust-toolchain.toml`: an exact stable release >= 1.88 (pinned at implementation), components `rustfmt`, `clippy`, `llvm-tools-preview`, target `wasm32-unknown-unknown`. `src/rustfmt.toml`: defaults plus the edition. `src/.cargo/config.toml`: only settings a build proves it needs; no registry or source overrides.
- `src/deny.toml`: bans `tokio < 1.54.1`, `h2 < 0.4.16`, `bytes < 1.11.1`, `rustls < 0.23.45`; `yanked = "deny"`; GPL-3.0-compatible licences only; sources crates.io only; each advisory ignore carries `reason = "added=YYYY-MM-DD; expires=YYYY-MM-DD; why=..."` (single ignore list, read by `audit.sh`).
- Root `.gitignore` adds `target/`, `*.log`, `**/logs/`, `e2e-results/`, `.env`, `secrets/`, `secrets-dev/`.

### Layout Rationale

- One Cargo workspace for all Rust code (ADR-001); source under `src/` (owner rule for new projects); `deploy/`, `scripts/ci/`, `tests/` at the root because Docker, CI and the black-box harness use the whole repository, not only the workspace.
- Component folders keep the doc numbers (`02_tree`) so specs, epics and code line up; packages follow E-00 (`sunnycloud-<name>`, `sc_<concern>`), except 08's `media-api` (08 spec, E-18). `contracts` holds every server port (ADR-015), so the graph is acyclic by construction; client traits stay in `sync-core` because `contracts` pulls server dependencies.
- Every cargo command runs with `src/` as working directory: `rust-toolchain.toml` and `.cargo/config.toml` are found from the current directory, not from `--manifest-path`.

## DTOs and Interfaces

Signatures come from the component descriptions (§ Ports); names from `canonical-names.md` § Ports and E-00 § Interface specification. This task creates the crates only; contracts parts 1-3 add the items.

### Shared DTOs

| DTO / type | Crate | Used by | Fields summary |
|---|---|---|---|
| `DbTx` | `sc_db` | `contracts` signatures; 01-08 | opaque; no public constructor (stub here, E-06 wraps the sqlx transaction) |
| `BlobRef` | `contracts` | 02, 03, 04, 08 | `blob_id, size, wrapped_key, master_key_id` (04 owns it) |
| `CommitActor`, `PublishEntry`, `EntryResult`, `PublishOutcome`, `EffectError` | `contracts` | 02, 03, 08 | `CommitActor{account_id, device_label}`; entry target existing or new, new-revision fields, `import_hint`, `vault: bool` (false in v1.0); `EffectError` is `Entry` or `Batch` |
| `LiveFile` | `contracts` | 02 -> 03, 06, 08, 11 | `kind, revision, blob_ref` |
| `EraseProgress` | `contracts` | 07 purger; hooks in 02, 03, 05, 06, 07a, 08, 20 | `remaining` |
| Job DTOs, `Probe`, `Plan`, `Params`, `OutputSpec`, `TranscodePolicy`, `decoder_protocol` messages | `media-shared` | 08 (API side of `:8081`), 09, 10 | wire and policy types only |
| `Cursor`, `ChangePage` (protocol DTOs); `SyncConfig`, `ConnectError` | `sync-core-proto` (E-23); `sync-core` | `sync-core`, `sync-core-wasm`; 14, `mobile_backup` | per 13 § 2; `ServerInfo` is defined in 11, OpenAPI is the authority |
| IPC request and response types; `BackupStatus` | `ipc_schema`; `mobile_backup` | 14, 15; Kotlin (17), Swift (18) | no IPC method returns a token; `BackupStatus` goes through UniFFI |

### Component Interfaces

| Owner | Interfaces (in `contracts` unless stated) | Main callers (full list: `module-layout.md`) |
|---|---|---|
| Registries | `Janitor`, `AccountLifecycleHook`, `BlobOwnerCleanup`, `DoctorCheck`, `HealthContributor`, `ReadinessCheck` | registered late by 12 |
| 01 identity | `Authenticator`, `PrincipalRevalidator`, `AccountGuard`, `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService`, `OperatorIdentityOps`, `AccountAdmin` | 11; 02 (`AccountGuard`); 05, 07, 12 |
| 02 tree | `TreeService`, `TreeReader`, `TreeCommit`, `TreeRestore`, `LiveUsageSource`, `TreeMaintenance`, `CommitEffect` (implemented by 03, 08) | 03, 06, 07, 08, 11, 12, 20 |
| 03 transfer | `UploadService`, `CommitService`, `DownloadService`, `ReservationSource` | 11; 02 (`CommitEffect`); 07, 12 |
| 04 blobstore | `BlobWriter`, `BlobReader`, `BlobRegistry`, `BlobCatalog`, `GcQueue`, `Scrubber`, `BlobInventory`, `MasterKeyring`, `EpochMarker` | 02, 03, 05, 08, 12 |
| 05 sync_feed | `FeedStamp`, `FeedNotifier`, `FeedHorizon`, `ChangeFeed`, `EpochAdmin` | 02 (stamp last), 11, 12 |
| 06 / 20 | `ShareService`, `PublicShareService`; `VaultService` (v1.2) | 11 |
| 07 / 07a | `AdminService`, `AccountService`, `AccountDeletionService`, `DevSeed`; `QuotaLedger`, `QuotaAdmin` | 11, 12; 02, 03 (`QuotaLedger`); 07 (`QuotaAdmin`) |
| 08 media_jobs | `ThumbnailStateSource`, `DerivedOpener`, `WorkerAuthenticator`, `WorkerJobs`, `MediaAdmin` | 02, 03, 11, 12 |
| 11 http_api | `HttpServer`, `ApiServices` (concrete, in 11) | 12 |
| 13 (`sync-core`) | `SyncApi` (`HttpSyncClient`), `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink` | 14, `mobile_backup`; hosts implement the last five |
| 14 / 17 / 10 | `DaemonControl` (`ipc_schema`); `BackupCore` (`mobile_backup`); `decoder_protocol` (`media-shared`) | 15 and tray; Kotlin/Swift; 09 |

## CI/CD Pipeline

| Stage (job) | Purpose | Trigger | Added by |
|---|---|---|---|
| `rust-lint` | `cargo fmt --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo sqlx prepare --check` | PR to `dev`, push | CI skeleton; prepare check by migrations setup |
| `rust-test` | `cargo test --workspace --locked` (never `-q`) against a PostgreSQL 18 service; `cargo llvm-cov` 75% overall, 90% critical paths | PR, push | CI skeleton |
| `pg18-migrations` | empty PG 18; up, down, up when files exist | PR, push | CI skeleton; upgrade snapshot and mid-way kill by E-06 |
| `supply-chain` | `scripts/ci/audit.sh`, `cargo deny check advisories bans licenses sources`, `LICENSE` present (`COPYING.iOS` check enabled when the owner supplies the text), wasm32 build of `sync-core-wasm` and `sc_ids` | PR, push | CI skeleton |
| `images` | build api, worker, decoder and the `-test` tags of api and worker (build only) | PR, push | CI skeleton; 404 test-clock check by E-21 |
| `web`, `openapi`, `mobile`, `desktop-build`, `core`, `ops-drill` | see `deployment/ci_cd_pipeline.md` | PR, push | E-26, E-21, E-27, E-24/E-25, E-31, E-22 |

### Pipeline Configuration Notes

- GitHub Actions is primary; YAML only checks out, caches and calls `scripts/ci/*.sh`, so the Woodpecker fallback (build and test only, file added when that remote exists) runs the same scripts.
- `uses:` pinned by full SHA with a version comment; images by digest; default `permissions: contents: read`; Dependabot for `github-actions`, `docker`, `cargo` now, `npm` and `pub` with E-26/E-27.
- Cargo steps use `working-directory: src` (so do the local pre-push commands of `ci_cd_pipeline.md`; its `cd web` becomes `cd src/web`); `Swatinem/rust-cache` keyed on `src/Cargo.lock` plus toolchain. `scripts/ci/watchdog.sh` wraps long steps (30 s tick, STALLED and aborted after 2 minutes with no output).
- `cargo sqlx prepare --check` needs `DATABASE_URL`, so its job needs the PG 18 service; the migrations-setup child picks `rust-lint` or `pg18-migrations`.
- Coverage thresholds follow `quality-thresholds`; the critical-path list starts empty and grows per epic. PR gate budget on the skeleton: under 15 minutes.
- Staging is the `stage` branch: push and sign `stage-<sha>` images (release workflows, after E-00); no deploy target exists, so agents do not poll.
- Open: `environment_strategy.md` puts a gitleaks secret scan in the PR gate, but neither `ci_cd_pipeline.md` nor E-00 lists the job. Proposal: a step of `supply-chain` in the CI skeleton child.

## Containers, compose and observability

| Image | Dockerfile | Runtime base | User | Health check | Ports |
|---|---|---|---|---|---|
| `sunnycloud-api` (+ `-test`) | `deploy/docker/api.Dockerfile` | `gcr.io/distroless/cc-debian13:nonroot@sha256:<pin>` | 10001 | `sunnycloud healthcheck` -> `:9090/readyz` | 8080, 8081, 9090; 9091 in `-test` only |
| `sunnycloud-worker` (+ `-test`) | `deploy/docker/worker.Dockerfile` | same | 10002 | `sunnycloud-worker healthcheck` | 9090 |
| `sunnycloud-decoder` | `deploy/docker/decoder.Dockerfile` | `debian:trixie-slim@sha256:<pin>` | 10003 | `decoder-runner health` | none (`network_mode: none`) |

- Multi-stage builds from the repository root: `rust:<stable>-trixie@sha256:<pin>` builder, cargo-chef, `cargo build --release --locked -p sunnycloud-server --features ${FEATURES:-}` with `SQLX_OFFLINE=true`. The api web stage arrives with E-26, the media stages with E-19. Until E-22 gives the binaries `serve` and `healthcheck`, SUT containers cannot report healthy; the checks are declared anyway and nothing fakes them.
- `.dockerignore` at the root: `.git`, `_docs`, `**/target`, `**/node_modules`, `**/e2e-results`, `**/.env*`, `secrets*`, `*.md` (`**/` is needed to match `src/target`).
- `deploy/compose/compose.dev.yml` (E-00 layout; `containerization.md` puts it at the root): db (Postgres 18, volume at `/var/lib/postgresql`, role init script), caddy (`tls internal`), migrate, api and worker with `FEATURES=test-clock`, both decoders, mock-idp. `tests/compose.test.yml` uses the services and networks of `tests/environment.md` with the operator-install names.
- Observability (`deployment/observability.md`, built by E-09): JSON logs to stdout; `GET :9090/healthz` (liveness), `GET :9090/readyz` (200 or 503 with failing checks), `GET :9090/metrics` (Prometheus, prefix `sunnycloud_`), backend network only. These replace the template's `/health/live` and `/health/ready`.

## Environment Strategy

| Environment | Purpose | Configuration notes |
|---|---|---|
| Development | workstation | `compose.dev.yml`, or `cargo run -p sunnycloud-server` against the compose db on `127.0.0.1:5432`; mock-idp; throw-away secrets in gitignored `secrets-dev/`; short durations |
| Test (CI) | PR gate | `tests/compose.test.yml`, SUT images by digest, `-test` api and worker, secrets generated per run, `sunnycloud.test` with the test CA |
| Staging | `stage` branch | signed `stage-<sha>` images; no shared host exists yet; short durations |
| Production | operator install | `deploy/compose/compose.yml` from the release bundle, digests from `images.lock`, operator secrets (mode 0400), production defaults, clock endpoints absent |

### Environment Variables

Rule: `SUNNYCLOUD_` + upper-cased key (helper 01 owns the table); an unknown `SUNNYCLOUD_*` refuses start; secrets only as files under `/run/secrets` (`<NAME>_FILE`).

| Variable | Dev | Staging | Production | Description |
|---|---|---|---|---|
| `PUBLIC_HOST` | `sunnycloud.localhost` | stack host name | operator DNS name | Caddy site and TLS name |
| `SUNNYCLOUD_PUBLIC_URL` | `https://sunnycloud.localhost` | `https://<host>` | `https://<PUBLIC_HOST>` | redirects, links, cookies; https outside dev |
| `SUNNYCLOUD_VERSION` | local build | `stage-<sha>` | release semver | image tag |
| `BLOB_PATH`, `SCRATCH_PATH` | Docker volumes | stack volumes | XFS mount; dedicated scratch mount or quota | blob volume; `decoder-general` scratch |
| `SUNNYCLOUD_TRUSTED_PROXIES` | Caddy address | Caddy address | `172.29.1.2` | unset means `X-Forwarded-For` ignored |
| `SUNNYCLOUD_INTERNAL_ADDR` | `0.0.0.0:8081` | `172.29.0.2:8081` | `172.29.0.2:8081` | worker listener, backend network only |
| `SUNNYCLOUD_BLOB_FREE_RESERVE`, `SUNNYCLOUD_DEFAULT_QUOTA` | small values | small values | `20GiB`, `100GiB` | free-space floor; per-user quota |
| `SUNNYCLOUD_GITHUB_CLIENT_ID`, `SUNNYCLOUD_OIDC_ISSUER`, `SUNNYCLOUD_OIDC_CLIENT_ID` | mock-idp | mock-idp | operator apps | at least one provider |
| Durations (`SUNNYCLOUD_RETENTION`, `_TOMBSTONE_HORIZON`, `_DEVICE_CODE_TTL`, `_JOB_LEASE`, ...) | short | short | defaults (`7d`, `90d`, `10m`, `5m`) | full table in `environment_strategy.md` |
| `WORKER_CPUS`, `WORKER_MEM` | `2`, `4g` | `2`, `4g` | at most half the cores; `4g` | `decoder-general` limits |
| Secret files `master_key`, `db_api_url`, `db_migrator_url`, `postgres_password`, `worker_api_token`, `share_grant_key`, `provider_*_client_secret`, `setup_token` | `secrets-dev/` | generated by CI | operator files | never in env files or git |

Build and test variables: `FEATURES` (`test-clock` only for `-test` images), `SQLX_OFFLINE`, `SUT_TAG`, `WORKER_CPUSET`. Optional keys (`SUNNYCLOUD_LOG_LEVEL`, `SUNNYCLOUD_SHARE_MAX_TTL`, ...) are listed in `environment_strategy.md`. Open: the ops listener (`:9090`) address key is not yet in helper 01's table (`containerization.md`). `deploy/compose/.env.example` carries its operator block verbatim (Dockerfiles/compose child).

## Database Migration Approach

**Migration tool**: sqlx 0.9, migrations embedded in the `sunnycloud` binary through `sc_db` (E-06).
**Strategy**: reversible pairs `YYYYMMDDHHMMSS_<verb>_<object>.up.sql` / `.down.sql` in `src/migrations/`; one transaction each under sqlx's advisory lock; the `<object>` is the owner's domain word plus at least one more word (`create_quota_table`, `module-layout.md` § Migrations); additive by default (expand/contract); applied files immutable; `src/.sqlx/` committed. Run only by the one-shot `migrate` service as `sunnycloud_migrator`; the API (`sunnycloud`, DML only) checks the version and refuses to serve on a mismatch. `deploy/postgres/initdb.d/10-roles.sh` creates both roles from secret files; grants come from migrations. No seed data in production; dev seed is `sunnycloud admin dev-seed`; tests use a golden snapshot.

### Initial Schema

E-00 ships zero migrations. Each owning epic adds its own pair (`data_model.md`):

| Owner (epic) | Tables |
|---|---|
| helper 04 audit (E-07) | `audit_log` (first real migration) |
| 04 blobstore (E-10); 07a quota (E-11); 05 sync_feed (E-12) | `blobs`, `blob_gc`; `quota`; `account_feeds` and `nodes.seq` with `nodes_feed_idx` |
| 02 tree (E-13); 03 transfer (E-14) | `nodes`, `revisions`; `upload_sessions`, `commit_receipts` |
| 01 identity (E-15) | `accounts`, `identities`, `sessions`, `device_tokens`, `device_grants`, `invites`, `oauth_states`, `login_codes` |
| 06 sharing (E-16); 08 media_jobs (E-18); 20 vault (E-30, v1.2) | `share_links`; `jobs`, `derived`, `worker_status`; `vaults` |
| 14 desktop (E-24) | SQLite `LocalIndex`: `meta`, `pair`, `entry`, `batch`, `upload_checkpoint` (forward-only, `PRAGMA user_version`) |

Decided (owner, 2026-11-04): the database epic E-06 (AZ-3638) writes a minimal `accounts` migration (id, state, created_at) before any tenant table; E-15 (identity) later adds its columns and the other identity tables. Migration order follows foreign-key order.

## Test Structure

```
src/<crate>/src/**              #[cfg(test)] unit tests
src/<crate>/tests/              crate integration tests; DB crates use real PG 18 (sc_template, one database per test)
src/server/tests/crate_graph.rs crate-graph test (this task)
tests/
├── run.sh  compose.test.yml     --profile core|web|desktop|...; reset (E-00)
├── consumer/                   Python 3.13, pytest -v --tb=short -p e2e_report -> e2e-results/report.csv (E-00)
├── tools/e2e-ctl/              host fault injector: skeleton in E-00, actions in E-31
└── mocks/ tools/ desktop/ lab/ idp and update mocks, shaper, perf client, lab runner (E-31)
src/web/ (vitest), src/mobile/ (flutter test)                                (E-26, E-27)
```

### Test Configuration Notes

- Crate-graph test (checks of `module-layout.md` § Allowed Dependencies): reads `cargo metadata` and fails on any edge between two crates under `src/components/`, any dependency on `sunnycloud-server`, a direct `sqlx` dependency in `sunnycloud-worker`, `decoder-runner`, `sunnycloud-http-api`, `sunnycloud-server` or `media-shared`, and any client crate (`sync-core*`, `mobile_backup`, `vault-core`, desktop crates) depending on `contracts` or a component (ADR-015). It lives beside the composition root, the one crate that may see every component.
- Black-box tests never import SUT code. Report columns and the `NOT-RUN(<code>):` rule follow `tests/environment.md`. The `e2e-ctl` skeleton answers a typed `not_implemented` error for every action, so no test can pass through it.
- No quiet runs anywhere (`cargo test` without `-q`, pytest `-v --tb=short`).
- Harness self-test (E-00 AC 7): until E-22 lets the SUT serve, `tests/run.sh --profile core` can only start the services that run (db, consumer). Which subset, and how the run reports the rest as `NOT-RUN`, is decided in the harness child.

## Implementation Order

E-00 children are ordered by real dependency: the CI jobs and the harness need migrations, Dockerfiles and `LICENSE` to exist first, and the `contracts` parts need the error, clock and ids helpers first (decompose Step 4 correction, 2026-11-04). The full order is `_docs/02_tasks/_dependencies_table.md`. Epics follow `epics.md`; ids are already in dependency order.

| Order | Component | Reason |
|---|---|---|
| 1 | This task (E-00 children 1 and 2) | everything builds on the workspace and crates |
| 2 | E-02 error, E-03 clock, E-04 ids/hashing (AZ-3764, AZ-3765, AZ-3766) | the port signatures use their value types (`ErrorCode`, `Timestamp`, `FileHash`, `RawToken`); they need only this task |
| 3-5 | `contracts` parts 1, 2, 3 (AZ-3768, AZ-3769, AZ-3770) | ports before any component can be faked; part 2 needs part 1 types |
| 6-7 | `media-shared` skeleton; `sync-core` and `sync-core-wasm` skeletons | need the workspace only; the second proves the wasm32 split |
| 8 | Migrations setup | `src/migrations/`, `src/.sqlx/` for the CI jobs |
| 9 | Dockerfiles, `.dockerignore`, `compose.dev.yml`, role init script | `images` job and harness need them |
| 10 | Test-harness skeleton | runs against the images from step 8 |
| 11 | `LICENSE` and README layout section | `supply-chain` checks `LICENSE` |
| 12 | CI skeleton | gates everything above; SHA pins, coverage gate |
| 13 | E-01, E-05 (AZ-3763, AZ-3767) | config; names (needs the clock). E-02..E-04 already ran at step 2 |
| 14 | E-06..E-09 (AZ-3638, AZ-3642, AZ-3620, AZ-3622) | db and migrations, audit, rate limit; observability last, it needs every helper |
| 15 | E-10..E-15 (AZ-3627, AZ-3631, AZ-3635, AZ-3640, AZ-3643, AZ-3646) | blobstore, quota, sync feed, tree, transfer, identity: the first vertical slice |
| 16 | E-16..E-20 (AZ-3619, AZ-3624, AZ-3629, AZ-3634, AZ-3639) | sharing, admin, media jobs, decoder, worker supervisor |
| 17 | E-21, E-22 (AZ-3644, AZ-3647) | http api, then the operator CLI (composition root) |
| 18 | E-23..E-26 (AZ-3650, AZ-3625, AZ-3628, AZ-3632) | sync core (adds `src/sync-core-proto/`), desktop daemon and settings, web app (creates `src/web`) |
| 19 | E-27..E-30 (AZ-3637, AZ-3641, AZ-3645, AZ-3648) | Android (creates `src/mobile`), iOS v1.1, vault core and service v1.2 |
| 20 | E-31 (AZ-3649) | black-box suite; harness tasks may start after E-00 and E-21 |

## Acceptance Criteria

**AC-1: Workspace builds and tests with `--locked`**
Given a clean checkout and the crate table of this task
When `cargo build --workspace --locked` and `cargo test --workspace --locked` run in `src/`, without `-q`
Then both exit 0, the test output lists each test by name, and `cargo metadata --no-deps` shows exactly the listed packages at their folders

**AC-2: Lint clean, silent discards rejected**
Given the skeleton
When `cargo fmt --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` run
Then both report 0 findings, and a seeded `let _ = <fn returning Result>;` makes clippy fail

**AC-3: No component-to-component edge**
Given the crate-graph test
When `cargo test -p sunnycloud-server --test crate_graph --locked` runs
Then it passes on the skeleton, and it fails when a seeded `sunnycloud-tree -> sunnycloud-quota` edge or a `sqlx` dependency in `media-shared` is added

**AC-4: No placeholder bodies; `DbTx` sealed**
Given `src/`
When it is searched for `todo!(` and `unimplemented!(`, and a `compile_fail` doc test constructs `sc_db::DbTx` from outside the crate
Then the search finds 0 hits, `contracts` compiles, and the doc test confirms there is no public constructor

**AC-5: wasm32 build**
Given the target from `rust-toolchain.toml`
When `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids` runs
Then it exits 0

**AC-6: Dependency floors enforced**
Given `src/deny.toml`
When `cargo deny check advisories bans licenses sources` runs on the skeleton, and `cargo deny check bans` runs on a branch that pins tokio 1.54.0 or h2 0.4.15
Then the first passes and the second fails

**AC-7: Ignored artefacts and no quiet runs**
Given the root `.gitignore`
When a build and a test run have finished
Then `git status` shows no `target/`, `*.log`, `e2e-results/`, `.env` or `secrets*/` entries, and no file in the repository runs `cargo test -q`, `--quiet` or `pytest -q`
